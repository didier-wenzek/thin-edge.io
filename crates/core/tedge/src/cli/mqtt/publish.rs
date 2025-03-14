use crate::cli::mqtt::MqttError;
use crate::command::Command;
use crate::error;
use crate::log::MaybeFancy;
use camino::Utf8PathBuf;
use certificate::parse_root_certificate;
use mqtt_channel::Topic;
use rumqttc::tokio_rustls::rustls::ClientConfig;
use rumqttc::tokio_rustls::rustls::RootCertStore;
use rumqttc::Event;
use rumqttc::Incoming;
use rumqttc::MqttOptions;
use rumqttc::Outgoing;
use rumqttc::Packet;
use rumqttc::QoS::AtLeastOnce;
use rumqttc::QoS::AtMostOnce;
use rumqttc::QoS::ExactlyOnce;
use tedge_config::tedge_toml::MqttAuthClientConfig;

const DEFAULT_QUEUE_CAPACITY: usize = 10;
use super::MAX_PACKET_SIZE;

pub struct MqttPublishCommand {
    pub host: String,
    pub port: u16,
    pub topic: Topic,
    pub message: String,
    pub qos: rumqttc::QoS,
    pub client_id: String,
    pub retain: bool,
    pub ca_file: Option<Utf8PathBuf>,
    pub ca_dir: Option<Utf8PathBuf>,
    pub client_auth_config: Option<MqttAuthClientConfig>,
}

impl Command for MqttPublishCommand {
    fn description(&self) -> String {
        format!(
            "publish the message \"{}\" on the topic \"{}\" with QoS \"{:?}\".",
            self.message,
            self.topic.as_ref(),
            self.qos
        )
    }

    fn execute(&self) -> Result<(), MaybeFancy<anyhow::Error>> {
        Ok(publish(self)?)
    }
}

fn publish(cmd: &MqttPublishCommand) -> Result<(), anyhow::Error> {
    let mut options = MqttOptions::new(cmd.client_id.as_str(), &cmd.host, cmd.port);
    options.set_clean_session(true);
    options.set_max_packet_size(MAX_PACKET_SIZE, MAX_PACKET_SIZE);

    if cmd.ca_file.is_some() || cmd.ca_dir.is_some() {
        let mut root_store = RootCertStore::empty();

        if let Some(ca_file) = &cmd.ca_file {
            parse_root_certificate::add_certs_from_file(&mut root_store, ca_file)?;
        }

        if let Some(ca_dir) = &cmd.ca_dir {
            parse_root_certificate::add_certs_from_directory(&mut root_store, ca_dir)?;
        }

        const INSECURE_MQTT_PORT: u16 = 1883;
        const SECURE_MQTT_PORT: u16 = 8883;

        if cmd.port == INSECURE_MQTT_PORT && !root_store.is_empty() {
            eprintln!("Warning: Connecting on port 1883 for insecure MQTT using a TLS connection");
        }
        if cmd.port == SECURE_MQTT_PORT && root_store.is_empty() {
            eprintln!("Warning: Connecting on port 8883 for secure MQTT with no CA certificates");
        }

        let tls_config = ClientConfig::builder().with_root_certificates(root_store);

        let tls_config = if let Some(client_auth) = cmd.client_auth_config.as_ref() {
            let client_cert = parse_root_certificate::read_cert_chain(&client_auth.cert_file)?;
            let client_key = parse_root_certificate::read_pvt_key(&client_auth.key_file)?;
            tls_config.with_client_auth_cert(client_cert, client_key)?
        } else {
            tls_config.with_no_client_auth()
        };

        options.set_transport(rumqttc::Transport::tls_with_config(tls_config.into()));
    }

    let payload = cmd.message.as_bytes();

    let (client, mut connection) = rumqttc::Client::new(options, DEFAULT_QUEUE_CAPACITY);
    super::disconnect_if_interrupted(client.clone(), None);
    let mut published = false;
    let mut acknowledged = false;
    let mut any_error = None;

    client.publish(&cmd.topic.name, cmd.qos, cmd.retain, payload)?;

    for event in connection.iter() {
        match event {
            Ok(Event::Outgoing(Outgoing::Publish(_))) => {
                published = true;
                if cmd.qos == AtMostOnce {
                    acknowledged = true;
                    break;
                }
            }
            Ok(Event::Incoming(Packet::PubAck(_))) => {
                if cmd.qos == AtLeastOnce {
                    acknowledged = true;
                    break;
                }
            }
            Ok(Event::Incoming(Packet::PubComp(_))) => {
                if cmd.qos == ExactlyOnce {
                    acknowledged = true;
                    break;
                }
            }
            Ok(Event::Incoming(Incoming::Disconnect)) => {
                any_error = Some(MqttError::ServerConnection("Disconnected".to_string()));
                break;
            }
            Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                break;
            }
            Err(err) => {
                any_error = Some(err.into());
                break;
            }
            _ => {}
        }
    }

    if !published {
        error!("the message has not been published");
    } else if !acknowledged {
        error!("the message has not been acknowledged");
    }

    client.disconnect()?;
    for event in connection.iter() {
        match event {
            Ok(Event::Outgoing(Outgoing::Disconnect)) | Err(_) => break,
            _ => {}
        }
    }

    if let Some(err) = any_error {
        Err(err.into())
    } else {
        Ok(())
    }
}
