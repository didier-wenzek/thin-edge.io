//! rustls connector for PKCS#11 devices.
//!
//! Reference:
//! - thin-edge: docs/src/references/hsm-support.md
//! - PKCS#11: https://docs.oasis-open.org/pkcs11/pkcs11-base/v2.40/os/pkcs11-base-v2.40-os.html

use asn1_rs::FromDer as _;
pub use cryptoki::types::AuthPin;

use anyhow::Context;
use asn1_rs::ToDer;
use camino::Utf8PathBuf;
use cryptoki::context::CInitializeArgs;
use cryptoki::context::Pkcs11;
use cryptoki::error::Error;
use cryptoki::mechanism::rsa::PkcsMgfType;
use cryptoki::mechanism::rsa::PkcsPssParams;
use cryptoki::mechanism::Mechanism;
use cryptoki::mechanism::MechanismType;
use cryptoki::object::Attribute;
use cryptoki::object::AttributeType;
use cryptoki::object::KeyType;
use cryptoki::object::ObjectHandle;
use cryptoki::session::Session;
use cryptoki::session::UserType;
use rustls::sign::Signer;
use rustls::sign::SigningKey;
use rustls::SignatureAlgorithm;
use rustls::SignatureScheme;
use tracing::debug;
use tracing::trace;
use tracing::warn;

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::Mutex;

// oIDs for curves defined here: https://datatracker.ietf.org/doc/html/rfc5480#section-2.1.1.1
// other can be browsed here: https://oid-base.com/get/1.3.132.0.34
const SECP256R1_OID: &str = "1.2.840.10045.3.1.7";
const SECP384R1_OID: &str = "1.3.132.0.34";
const SECP521R1_OID: &str = "1.3.132.0.35";

#[derive(Clone)]
pub struct CryptokiConfigDirect {
    pub module_path: Utf8PathBuf,
    pub pin: AuthPin,
    pub uri: Option<Arc<str>>,
}

impl Debug for CryptokiConfigDirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptokiConfigDirect")
            .field("module_path", &self.module_path)
            .field("pin", &"[REDACTED]")
            .field("uri", &self.uri)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct Cryptoki {
    context: Pkcs11,
    config: CryptokiConfigDirect,
}

impl Cryptoki {
    pub fn new(config: CryptokiConfigDirect) -> anyhow::Result<Self> {
        debug!(module_path = %config.module_path, "Loading PKCS#11 module");
        // can fail with Pkcs11(GeneralError, GetFunctionList) if P11_KIT_SERVER_ADDRESS is wrong
        let pkcs11client = match Pkcs11::new(&config.module_path) {
            Ok(p) => p,
            // i want to get inner error but i don't know if there is a better way to do this
            Err(Error::LibraryLoading(e)) => {
                return Err(e).context("Failed to load PKCS#11 dynamic object");
            }
            Err(e) => {
                return Err(e).context("Failed to load PKCS#11 dynamic object");
            }
        };

        pkcs11client.initialize(CInitializeArgs::OsThreads)?;

        Ok(Self {
            context: pkcs11client,
            config,
        })
    }

    pub fn signing_key(&self, uri: Option<&str>) -> anyhow::Result<Pkcs11SigningKey> {
        let mut config_uri = self
            .config
            .uri
            .as_deref()
            .map(|u| uri::Pkcs11Uri::parse(u).context("Failed to parse config PKCS#11 URI"))
            .transpose()?
            .unwrap_or_default();

        let request_uri = uri
            .map(|uri| uri::Pkcs11Uri::parse(uri).context("Failed to parse PKCS #11 URI"))
            .transpose()?
            .unwrap_or_default();

        config_uri.append_attributes(request_uri);
        let uri_attributes = config_uri;

        let wanted_label = uri_attributes.token.as_ref();
        let wanted_serial = uri_attributes.serial.as_ref();

        let slots_with_tokens = self.context.get_slots_with_token()?;
        let tokens: Result<Vec<_>, _> = slots_with_tokens
            .iter()
            .map(|s| {
                self.context
                    .get_token_info(*s)
                    .context("Failed to get slot info")
            })
            .collect();
        let tokens = tokens?;

        // if token/serial attributes are passed, find a token that has these attributes, otherwise any token will do
        let mut tokens = slots_with_tokens
            .into_iter()
            .zip(tokens)
            .filter(|(_, t)| wanted_label.is_none() || wanted_label.is_some_and(|l| t.label() == l))
            .filter(|(_, t)| {
                wanted_serial.is_none() || wanted_serial.is_some_and(|s| t.serial_number() == s)
            });
        let (slot, _) = tokens
            .next()
            .context("Didn't find a slot to use. The device may be disconnected.")?;

        let slot_info = self.context.get_slot_info(slot)?;
        let token_info = self.context.get_token_info(slot)?;
        debug!(?slot_info, ?token_info, "Selected slot");

        let session = self.context.open_ro_session(slot)?;
        session.login(UserType::User, Some(&self.config.pin))?;
        let session_info = session.get_session_info()?;
        debug!(?session_info, "Opened a readonly session");

        // get the signing key
        let key = Self::find_key_by_attributes(&uri_attributes, &session)?;
        let key_type = session
            .get_attributes(key, &[AttributeType::KeyType])?
            .into_iter()
            .next()
            .context("no keytype attribute")?;

        let Attribute::KeyType(keytype) = key_type else {
            anyhow::bail!("can't get key type");
        };

        let pkcs11 = PKCS11 {
            session: Arc::new(Mutex::new(session)),
        };

        // we need to select a signature scheme to use with a key - each type of key can only have one signature scheme
        // ideally we'd simply get a cryptoki mechanism that corresponds to this sigscheme but it's not possible;
        // instead we have to manually parse additional attributes to select a proper sigscheme; currently don't do it
        // and just select the most common sigscheme for both types of keys

        // NOTE: cryptoki has AttributeType::AllowedMechanisms, but when i use it in get_attributes() with opensc-pkcs11
        // module it gets ignored (not present or supported) and with softhsm2 module it panics(seems to be an issue
        // with cryptoki, but regardless):

        // thread 'main' panicked at library/core/src/panicking.rs:218:5:
        // unsafe precondition(s) violated: slice::from_raw_parts requires the pointer to be aligned and non-null, and the total size of the slice not to exceed `isize::MAX`
        // note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
        // thread caused non-unwinding panic. aborting.
        // Aborted (core dumped)

        let key = match keytype {
            KeyType::EC => {
                let sigscheme = get_ec_mechanism(&pkcs11.session.lock().unwrap(), key)
                    .unwrap_or(SigScheme::EcdsaNistp256Sha256);

                Pkcs11SigningKey {
                    session: pkcs11,
                    handle: key,
                    sigscheme,
                }
            }
            KeyType::RSA => Pkcs11SigningKey {
                session: pkcs11,
                handle: key,
                sigscheme: SigScheme::RsaPssSha256,
            },
            _ => anyhow::bail!("unsupported key type"),
        };

        Ok(key)
    }

    fn find_key_by_attributes(
        uri: &uri::Pkcs11Uri,
        session: &Session,
    ) -> anyhow::Result<ObjectHandle> {
        let mut key_template = vec![
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::Sign(true),
        ];
        if let Some(object) = &uri.object {
            key_template.push(Attribute::Label(object.as_bytes().to_vec()));
        }
        if let Some(id) = &uri.id {
            key_template.push(Attribute::Id(id.clone()));
        }

        trace!(?key_template, "Finding a key");

        let mut keys = session
            .find_objects(&key_template)
            .context("Failed to find private key objects")?
            .into_iter();

        let key = keys.next().context("Failed to find a private key")?;
        if keys.len() > 0 {
            warn!("Multiple keys were found. If the wrong one was chosen, please use a URI that uniquely identifies a key.")
        }

        Ok(key)
    }
}

#[derive(Debug, Clone)]
pub struct Pkcs11SigningKey {
    session: PKCS11,
    handle: ObjectHandle,
    sigscheme: SigScheme,
}

impl SigningKey for Pkcs11SigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        debug!("Offered signature schemes. offered={:?}", offered);
        let key_scheme = self.sigscheme.into();
        if offered.contains(&key_scheme) {
            debug!("Matching scheme: {key_scheme:?}");
            Some(Box::new(PkcsSigner {
                pkcs11: self.session.clone(),
                key: self.handle,
                sigscheme: self.sigscheme,
            }))
        } else {
            None
        }
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        self.sigscheme.into()
    }
}

#[derive(Debug, Clone)]
pub struct PKCS11 {
    pub session: Arc<Mutex<Session>>,
}

#[derive(Debug)]
pub struct PkcsSigner {
    pkcs11: PKCS11,
    key: ObjectHandle,
    sigscheme: SigScheme,
}

impl PkcsSigner {
    pub fn from_key(key: Pkcs11SigningKey) -> Self {
        Self {
            pkcs11: key.session,
            key: key.handle,
            sigscheme: key.sigscheme,
        }
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
        let session = self.pkcs11.session.lock().unwrap();

        let mechanism = self.sigscheme.into();
        let (mechanism, digest_mechanism) = match mechanism {
            Mechanism::EcdsaSha256 => (Mechanism::Ecdsa, Some(Mechanism::Sha256)),
            Mechanism::EcdsaSha384 => (Mechanism::Ecdsa, Some(Mechanism::Sha384)),
            Mechanism::EcdsaSha512 => (Mechanism::Ecdsa, Some(Mechanism::Sha512)),
            Mechanism::Sha1RsaPkcs => (Mechanism::RsaPkcs, Some(Mechanism::Sha1)),
            Mechanism::Sha256RsaPkcs => (Mechanism::RsaPkcs, Some(Mechanism::Sha256)),
            Mechanism::Sha384RsaPkcs => (Mechanism::RsaPkcs, Some(Mechanism::Sha384)),
            Mechanism::Sha512RsaPkcs => (Mechanism::RsaPkcs, Some(Mechanism::Sha512)),
            Mechanism::Sha256RsaPkcsPss(p) => (Mechanism::Sha256RsaPkcsPss(p), None),
            Mechanism::Sha384RsaPkcsPss(p) => (Mechanism::Sha384RsaPkcsPss(p), None),
            Mechanism::Sha512RsaPkcsPss(p) => (Mechanism::Sha512RsaPkcsPss(p), None),
            _ => {
                warn!(?mechanism, "Unsupported mechanism, trying it out anyway.");
                (Mechanism::Ecdsa, Some(Mechanism::Sha256))
            }
        };

        let direct_sign = digest_mechanism.is_none();

        trace!(input_message = %String::from_utf8_lossy(message), len=message.len(), ?mechanism, direct_sign);

        let digest;
        let to_sign = if direct_sign {
            message
        } else {
            digest = session
                .digest(&digest_mechanism.unwrap(), message)
                .context("pkcs11: Failed to digest message")?;
            &digest
        };

        trace!(?mechanism, "Session::sign");
        let signature_raw = session
            .sign(&mechanism, self.key, to_sign)
            .context("pkcs11: Failed to sign message")?;

        // Split raw signature into r and s values (assuming 32 bytes each)
        trace!("Signature (raw) len={:?}", signature_raw.len());
        let signature_asn1 = match mechanism {
            Mechanism::Ecdsa => {
                let size = signature_raw.len() / 2;
                let r_bytes = &signature_raw[0..size];
                let s_bytes = &signature_raw[size..];

                format_asn1_ecdsa_signature(r_bytes, s_bytes)
                    .context("pkcs11: Failed to format signature")?
            }

            _ => signature_raw,
        };
        trace!(
            "Encoded ASN.1 Signature: len={:?} {:?}",
            signature_asn1.len(),
            signature_asn1
        );
        Ok(signature_asn1)
    }
}

/// Currently supported signature schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SigScheme {
    EcdsaNistp256Sha256,
    EcdsaNistp384Sha384,
    EcdsaNistp521Sha512,
    RsaPssSha256,
}

impl From<SigScheme> for rustls::SignatureScheme {
    fn from(value: SigScheme) -> Self {
        match value {
            SigScheme::EcdsaNistp256Sha256 => Self::ECDSA_NISTP256_SHA256,
            SigScheme::EcdsaNistp384Sha384 => Self::ECDSA_NISTP384_SHA384,
            SigScheme::EcdsaNistp521Sha512 => Self::ECDSA_NISTP521_SHA512,
            SigScheme::RsaPssSha256 => Self::RSA_PSS_SHA256,
        }
    }
}

impl From<SigScheme> for rustls::SignatureAlgorithm {
    fn from(value: SigScheme) -> Self {
        match value {
            SigScheme::EcdsaNistp256Sha256
            | SigScheme::EcdsaNistp384Sha384
            | SigScheme::EcdsaNistp521Sha512 => Self::ECDSA,
            SigScheme::RsaPssSha256 => Self::RSA,
        }
    }
}

impl From<SigScheme> for Mechanism<'_> {
    fn from(value: SigScheme) -> Self {
        match value {
            SigScheme::EcdsaNistp256Sha256 => Self::EcdsaSha256,
            SigScheme::EcdsaNistp384Sha384 => Self::EcdsaSha384,
            SigScheme::EcdsaNistp521Sha512 => Self::EcdsaSha512,
            SigScheme::RsaPssSha256 => Mechanism::Sha256RsaPkcsPss(PkcsPssParams {
                hash_alg: MechanismType::SHA256,
                mgf: PkcsMgfType::MGF1_SHA256,
                // RFC8446 4.2.3: RSASSA-PSS PSS algorithms: [...] The length of
                // the Salt MUST be equal to the length of the digest algorithm
                // SHA256: 256 bits = 32 bytes
                s_len: 32.into(),
            }),
        }
    }
}

impl Signer for PkcsSigner {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        Self::sign(self, message).map_err(|e| rustls::Error::General(e.to_string()))
    }

    fn scheme(&self) -> SignatureScheme {
        self.sigscheme.into()
    }
}

fn format_asn1_ecdsa_signature(r_bytes: &[u8], s_bytes: &[u8]) -> Result<Vec<u8>, anyhow::Error> {
    let mut writer = Vec::new();

    write_asn1_integer(&mut writer, r_bytes);

    write_asn1_integer(&mut writer, s_bytes);

    let seq = asn1_rs::Sequence::new(writer.into());
    let b = seq.to_der_vec().unwrap();
    Ok(b)
}

fn write_asn1_integer(writer: &mut dyn std::io::Write, b: &[u8]) {
    let mut i = asn1_rs::BigInt::from_signed_bytes_be(b);
    if i.sign() == asn1_rs::Sign::Minus {
        // Prepend a most significant zero byte if value < 0
        let mut positive = b.to_vec();
        positive.insert(0, 0);

        i = asn1_rs::BigInt::from_signed_bytes_be(&positive);
    }
    let i = i.to_signed_bytes_be();
    let i = asn1_rs::Integer::new(&i);
    let _ = i.write_der(writer);
}

fn get_ec_mechanism(session: &Session, key: ObjectHandle) -> anyhow::Result<SigScheme> {
    let key_params = &[AttributeType::EcParams];
    let attrs = session
        .get_attributes(key, key_params)
        .context("Failed to get key params")?;
    trace!(?attrs);

    let attr = attrs
        .into_iter()
        .next()
        .context("Failed to get EcParams attribute")?;
    let Attribute::EcParams(ecparams) = attr else {
        anyhow::bail!("Failed to get EcParams attribute");
    };

    // this can be oid, but also a bunch of other things
    // https://docs.oasis-open.org/pkcs11/pkcs11-curr/v3.0/os/pkcs11-curr-v3.0-os.html#_Toc30061181
    let (_, ecparams) = asn1_rs::Any::from_der(&ecparams).context("Failed to parse EC_PARAMS")?;
    let oid = ecparams.as_oid().context("EC_PARAMS isn't an oID")?;
    let oid = oid.to_id_string();
    match oid.as_str() {
        SECP256R1_OID => Ok(SigScheme::EcdsaNistp256Sha256),
        SECP384R1_OID => Ok(SigScheme::EcdsaNistp384Sha384),
        SECP521R1_OID => Ok(SigScheme::EcdsaNistp521Sha512),
        _ => anyhow::bail!("Parsed oID({oid}) doesn't match any supported EC curve"),
    }
}

pub mod uri {
    use std::borrow::Cow;
    use std::collections::HashMap;

    /// Attributes decoded from a PKCS #11 URL.
    ///
    /// Attributes only relevant to us shall be put into fields and the rest is in `other` hashmap.
    ///
    /// https://www.rfc-editor.org/rfc/rfc7512.html
    #[derive(Debug, Clone, Default)]
    pub struct Pkcs11Uri<'a> {
        pub token: Option<Cow<'a, str>>,
        pub serial: Option<Cow<'a, str>>,
        pub id: Option<Vec<u8>>,
        pub object: Option<Cow<'a, str>>,
        pub other: HashMap<&'a str, Cow<'a, str>>,
    }

    impl<'a> Pkcs11Uri<'a> {
        pub fn parse(uri: &'a str) -> anyhow::Result<Self> {
            let path = uri
                .strip_prefix("pkcs11:")
                .ok_or_else(|| anyhow::anyhow!("missing PKCS #11 URI scheme"))?;

            // split of the query component
            let path = path.split_once('?').map(|(l, _)| l).unwrap_or(path);

            // parse attributes, duplicate attributes are an error (RFC section 2.3)
            let pairs_iter = path.split(';').filter_map(|pair| pair.split_once('='));
            let mut pairs: HashMap<&str, &str> = HashMap::new();
            for (k, v) in pairs_iter {
                let prev_value = pairs.insert(k, v);
                if prev_value.is_some() {
                    anyhow::bail!("PKCS#11 URI contains duplicate attribute ({k})");
                }
            }

            let token = pairs
                .remove("token")
                .map(|v| percent_encoding::percent_decode_str(v).decode_utf8_lossy());
            let serial = pairs
                .remove("serial")
                .map(|v| percent_encoding::percent_decode_str(v).decode_utf8_lossy());
            let object = pairs
                .remove("object")
                .map(|v| percent_encoding::percent_decode_str(v).decode_utf8_lossy());

            let id: Option<Vec<u8>> = pairs
                .remove("id")
                .map(|id| percent_encoding::percent_decode_str(id).collect());

            let other = pairs
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        percent_encoding::percent_decode_str(v).decode_utf8_lossy(),
                    )
                })
                .collect();

            Ok(Self {
                token,
                serial,
                id,
                object,
                other,
            })
        }

        /// Add new attributes from `other` to `self`.
        ///
        /// If other contains new attributes not present in self, add them to self. If these
        /// attributes are already present in self, preserve value currently in self.
        pub fn append_attributes(&mut self, other: Self) {
            self.token = self.token.take().or(other.token);
            self.serial = self.serial.take().or(other.serial);
            self.id = self.id.take().or(other.id);
            self.object = self.object.take().or(other.object);

            for (attribute, value) in other.other {
                if !self.other.contains_key(attribute) {
                    self.other.insert(attribute, value);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn decodes_valid_pkcs11_uri() {
            // test input URIs taken from RFC examples section and combined with properties we actually use
            // https://www.rfc-editor.org/rfc/rfc7512.html#section-3
            let input = "pkcs11:token=The%20Software%20PKCS%2311%20Softtoken;\
            manufacturer=Snake%20Oil,%20Inc.;\
            model=1.0;\
            object=my-certificate;\
            type=cert;\
            id=%69%95%3E%5C%F4%BD%EC%91;\
            serial=\
            ?pin-source=file:/etc/token_pin";

            let attributes = Pkcs11Uri::parse(input).unwrap();

            assert_eq!(attributes.token.unwrap(), "The Software PKCS#11 Softtoken");
            assert_eq!(
                attributes.other.get("manufacturer").unwrap(),
                "Snake Oil, Inc."
            );
            assert_eq!(attributes.other.get("model").unwrap(), "1.0");
            assert_eq!(attributes.serial.unwrap(), "");
            assert_eq!(attributes.object.unwrap(), "my-certificate");
            assert_eq!(
                attributes.id,
                Some(vec![0x69, 0x95, 0x3e, 0x5c, 0xf4, 0xbd, 0xec, 0x91])
            );
        }

        #[test]
        fn fails_on_uris_with_duplicate_attributes() {
            let input = "pkcs11:token=my-token;token=my-token";
            let err = Pkcs11Uri::parse(input).unwrap_err();
            assert!(err
                .to_string()
                .contains("PKCS#11 URI contains duplicate attribute (token)"));
        }

        #[test]
        fn fails_on_uris_with_invalid_scheme() {
            let input = "not a pkcs#11 uri";
            let err = Pkcs11Uri::parse(input).unwrap_err();
            assert!(err.to_string().contains("missing PKCS #11 URI scheme"));
        }

        #[test]
        fn appends_attributes_correctly() {
            let mut uri1 = Pkcs11Uri::parse("pkcs11:token=token1").unwrap();
            let uri2 = Pkcs11Uri::parse(
                "pkcs11:token=token2;serial=serial2;id=%01%02;object=object2;key1=value1",
            )
            .unwrap();

            uri1.append_attributes(uri2);

            assert_eq!(uri1.token.unwrap(), "token1");
            assert_eq!(uri1.serial.unwrap(), "serial2");
            assert_eq!(uri1.id, Some(vec![0x01, 0x02]));
            assert_eq!(uri1.object.unwrap(), "object2");
            assert_eq!(uri1.other.get("key1").unwrap(), "value1");
        }

        #[test]
        fn appends_attributes_with_no_conflicts() {
            let mut uri1 = Pkcs11Uri::parse("pkcs11:").unwrap();
            let uri2 = Pkcs11Uri::parse(
                "pkcs11:token=token2;serial=serial2;id=%01%02;object=object2;key1=value1",
            )
            .unwrap();

            uri1.append_attributes(uri2);

            assert_eq!(uri1.token.unwrap(), "token2");
            assert_eq!(uri1.serial.unwrap(), "serial2");
            assert_eq!(uri1.id, Some(vec![0x01, 0x02]));
            assert_eq!(uri1.object.unwrap(), "object2");
            assert_eq!(uri1.other.get("key1").unwrap(), "value1");
        }

        #[test]
        fn does_not_override_existing_attributes() {
            let mut uri1 = Pkcs11Uri::parse(
                "pkcs11:token=token1;serial=serial1;id=%01;object=object1;key1=value1",
            )
            .unwrap();
            let uri2 = Pkcs11Uri::parse(
                "pkcs11:token=token2;serial=serial2;id=%02;object=object2;key2=value2",
            )
            .unwrap();

            uri1.append_attributes(uri2);

            assert_eq!(uri1.token.unwrap(), "token1");
            assert_eq!(uri1.serial.unwrap(), "serial1");
            assert_eq!(uri1.id, Some(vec![0x01]));
            assert_eq!(uri1.object.unwrap(), "object1");
            assert_eq!(uri1.other.get("key1").unwrap(), "value1");
            assert_eq!(uri1.other.get("key2").unwrap(), "value2");
        }
    }
}
