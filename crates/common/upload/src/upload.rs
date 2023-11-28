use crate::error::UploadError;
use backoff::future::retry_notify;
use backoff::ExponentialBackoff;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use log::info;
use log::warn;
use reqwest::header::CONTENT_LENGTH;
use reqwest::header::CONTENT_TYPE;
use reqwest::Body;
use reqwest::Identity;
use std::fmt::Display;
use std::fmt::Formatter;
use std::time::Duration;
use tokio::fs::File;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;

fn default_backoff() -> ExponentialBackoff {
    // Default retry is an exponential retry with a limit of 5 minutes total.
    // Let's set some more reasonable retry policy so we don't block the uploads for too long.
    ExponentialBackoff {
        initial_interval: Duration::from_secs(15),
        max_elapsed_time: Some(Duration::from_secs(300)),
        randomization_factor: 0.1,
        ..Default::default()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ContentType {
    TextPlain,
    ApplicationOctetStream,
}

impl Display for ContentType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentType::TextPlain => write!(f, "text/plain"),
            ContentType::ApplicationOctetStream => write!(f, "application/octet-stream"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UploadInfo {
    pub url: String,
    pub auth: Option<Auth>,
    pub content_type: ContentType,
}

impl From<&str> for UploadInfo {
    fn from(url: &str) -> Self {
        Self::new(url)
    }
}

impl UploadInfo {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            auth: None,
            content_type: ContentType::ApplicationOctetStream,
        }
    }

    pub fn with_auth(self, auth: Auth) -> Self {
        Self {
            auth: Some(auth),
            ..self
        }
    }

    pub fn with_content_type(self, content_type: ContentType) -> Self {
        Self {
            content_type,
            ..self
        }
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    Bearer(String),
}

#[derive(Debug)]
pub struct Uploader {
    source_filename: Utf8PathBuf,
    backoff: ExponentialBackoff,
    identity: Option<Identity>,
}

impl Uploader {
    pub fn new(target_path: Utf8PathBuf, identity: Option<Identity>) -> Self {
        Self {
            source_filename: target_path,
            backoff: default_backoff(),
            identity,
        }
    }

    pub fn set_backoff(&mut self, backoff: ExponentialBackoff) {
        self.backoff = backoff;
    }

    pub async fn upload(&self, url: &UploadInfo) -> Result<(), UploadError> {
        self.upload_request(url).await?;

        Ok(())
    }

    async fn upload_request(&self, url: &UploadInfo) -> Result<reqwest::Response, UploadError> {
        use crate::error::ErrContext;

        let operation = || async {
            let file = File::open(&self.source_filename)
                .await
                .context(format!("Can't open a file {:?}", &self.source_filename))
                .map_err(backoff::Error::Permanent)?;

            let file_length = file
                .metadata()
                .await
                .context(format!(
                    "Can't read a file {:?} metadata",
                    &self.source_filename
                ))
                .map_err(backoff::Error::Permanent)?
                .len();

            let file_body = Body::wrap_stream(FramedRead::new(file, BytesCodec::new()));

            let mut client = reqwest::Client::builder();
            if let Some(identity) = self.identity.clone() {
                client = client.identity(identity);
            }
            let client = client
                .build()
                .map_err(UploadError::from)
                .map_err(backoff::Error::Permanent)?;

            // If HTTPS is enabled for the file transfer service, the response to an HTTP request
            // will be a temporary redirect. We can't retry the PUT request, so we first perform a
            // HEAD request to establish the correct URL
            let head_res = client.head(url.url()).send().await;
            let head_res_url = match &head_res {
                Ok(res) => Some(res.url()),
                Err(err) => {
                    // e.g. if we need a client certificate but haven't provided one
                    // We handle this error here because if there is a certificate error now
                    // there is guaranteed to be one later
                    if axum_tls::rustls_error_from_reqwest(err).is_some() {
                        return Err(backoff::Error::Permanent(head_res.unwrap_err().into()));
                    }
                    err.url()
                }
            };
            let target_url = head_res_url.map_or(url.url(), |u| u.as_str());

            if target_url != url.url() {
                info!("Redirecting request from {} to {target_url}", url.url())
            }

            // Todo: Ideally it detects the appropriate content-type automatically, e.g. UTF-8 => text/plain
            let mut client = client
                .put(target_url)
                .header(CONTENT_TYPE, url.content_type.to_string())
                .header(CONTENT_LENGTH, file_length);

            if let Some(Auth::Bearer(token)) = &url.auth {
                client = client.bearer_auth(token)
            }

            client
                .body(file_body)
                .send()
                .await
                .map_err(|err| {
                    if err.is_builder() || err.is_connect() {
                        backoff::Error::Permanent(UploadError::Network(err))
                    } else {
                        backoff::Error::transient(UploadError::Network(err))
                    }
                })?
                .error_for_status()
                .map_err(|err| match err.status() {
                    Some(status_error) if status_error.is_client_error() => {
                        backoff::Error::Permanent(UploadError::Network(err))
                    }
                    _ => backoff::Error::transient(UploadError::Network(err)),
                })
        };

        retry_notify(self.backoff.clone(), operation, |err, dur: Duration| {
            let dur = dur.as_secs();
            warn!("Temporary failure: {err}. Retrying in {dur}s",)
        })
        .await
    }

    pub fn filename(&self) -> &Utf8Path {
        self.source_filename.as_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::BodyStream;
    use axum::http::StatusCode;
    use axum::routing::put;
    use axum::Router;
    use backoff::ExponentialBackoffBuilder;
    use futures::future::pending;
    use futures::stream::StreamExt;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use tedge_test_utils::fs::TempTedgeDir;
    use tempfile::tempdir;
    use tokio::fs::read_to_string;
    use tokio::io::AsyncWriteExt;
    use tokio::io::BufWriter;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn upload_content_no_auth() {
        let mut server = mockito::Server::new();
        let _mock1 = server
            .mock("PUT", "/some_file.txt")
            .with_status(201)
            .create();

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = UploadInfo::new(&target_url);

        let ttd = TempTedgeDir::new();
        ttd.file("file_upload.txt")
            .with_raw_content("Hello, world!");

        let mut uploader = Uploader::new(ttd.utf8_path().join("file_upload.txt"), None);
        uploader.set_backoff(ExponentialBackoff {
            current_interval: Duration::ZERO,
            ..Default::default()
        });

        assert!(uploader.upload(&url).await.is_ok())
    }

    #[tokio::test]
    async fn upload_content_with_auth() {
        let mut server = mockito::Server::new();
        let _mock1 = server
            .mock("PUT", "/some_file.txt")
            .with_status(201)
            .match_header(
                "Authorization",
                mockito::Matcher::Regex(r"Bearer .*".to_string()),
            )
            .create();

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = UploadInfo::new(&target_url).with_auth(Auth::Bearer("1234".to_string()));

        let ttd = TempTedgeDir::new();
        ttd.file("file_upload.txt")
            .with_raw_content("Hello, world!");

        let mut uploader = Uploader::new(ttd.utf8_path().join("file_upload.txt"), None);

        uploader.set_backoff(ExponentialBackoff {
            current_interval: Duration::ZERO,
            ..Default::default()
        });

        assert!(uploader.upload(&url).await.is_ok())
    }

    #[tokio::test]
    async fn upload_content_from_file_that_does_not_exist() {
        let mut server = mockito::Server::new();
        let _mock1 = server
            .mock("PUT", "/some_file.txt")
            .with_status(201)
            .create();

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = UploadInfo::new(&target_url);

        // Not existing filename
        let source_path = Utf8Path::new("not_exist.txt").to_path_buf();

        let uploader = Uploader::new(source_path, None);
        assert!(uploader.upload(&url).await.is_err());
    }

    #[test]
    fn default_uploader_uses_customised_backoff_parameters() {
        let uploader = Uploader::new(Utf8PathBuf::default(), None);

        assert_eq!(uploader.backoff.initial_interval, Duration::from_secs(15));
        assert_eq!(
            uploader.backoff.max_elapsed_time,
            Some(Duration::from_secs(300))
        );
        assert_eq!(uploader.backoff.randomization_factor, 0.1);
    }

    #[tokio::test]
    async fn retry_upload_when_disconnected() {
        use anyhow::Context;
        let temp_dir = Arc::new(tempdir().unwrap());

        let listener = TcpListener::bind("localhost:0").await.unwrap();

        let port = listener.local_addr().unwrap().port();

        let target_path = Arc::new(
            Utf8Path::from_path(temp_dir.path())
                .unwrap()
                .join("target.txt"),
        );
        let target_path_clone = target_path.clone();
        let is_first_attempt = Arc::new(AtomicBool::new(true));
        let (io_err_tx, mut io_err_rx) = mpsc::channel::<anyhow::Error>(1);

        let app = Router::new().route(
            "/target.txt",
            put(|mut body: BodyStream| async move {
                let res = async {
                    if is_first_attempt.fetch_and(false, Ordering::SeqCst) {
                        Ok(StatusCode::INTERNAL_SERVER_ERROR)
                    } else {
                        let mut file = BufWriter::new(
                            File::create(target_path_clone.as_path())
                                .await
                                .context("creating file")?,
                        );
                        while let Some(chunk) = body.next().await {
                            file.write_all(&chunk.context("receiving chunk")?)
                                .await
                                .context("writing chunk")?;
                        }
                        Ok(StatusCode::CREATED)
                    }
                }
                .await;

                match res {
                    Ok(status_code) => status_code,
                    Err(err) => {
                        io_err_tx.send(err).await.unwrap();
                        // If we've encountered a server error, don't respond
                        // The uploader will keep running, so the main task will see the error
                        // message on the channel and panic accordingly
                        pending().await
                    }
                }
            }),
        );

        let server_task = tokio::spawn(
            axum::Server::from_tcp(listener.into_std().unwrap())
                .unwrap()
                .serve(app.into_make_service()),
        );

        tokio::time::sleep(Duration::from_millis(50)).await;

        let source_path = Utf8Path::from_path(temp_dir.path())
            .unwrap()
            .join("source.txt");

        let mut source_file = File::create(&source_path).await.unwrap();

        write_to_file_with_size(&mut source_file, 1024 * 1024).await;

        let mut uploader = Uploader::new(source_path.to_owned(), None);
        // Adjust the backoff to be super fast for testing purposes
        uploader.set_backoff(
            ExponentialBackoffBuilder::new()
                .with_initial_interval(Duration::from_millis(10))
                .with_max_elapsed_time(Some(Duration::from_secs(10)))
                .build(),
        );
        let url = UploadInfo::new(&format!("http://localhost:{port}/target.txt"));

        tokio::select! {
            upload_res = uploader.upload(&url) => upload_res.unwrap(),
            server_err = io_err_rx.recv() => panic!("{:?}", server_err),
        };

        server_task.abort();

        let target_content = read_to_string(target_path.as_path()).await.unwrap();
        let source_content = read_to_string(source_path).await.unwrap();

        assert_eq!(source_content.len(), target_content.len());
        assert_eq!(source_content, target_content);
    }

    async fn write_to_file_with_size(file: &mut File, size: usize) {
        let data: String = "Some data!".into();
        let loops = size / data.len();
        let mut buffer = String::with_capacity(size);
        for _ in 0..loops {
            buffer.push_str("Some data!");
        }

        file.write_all(buffer.as_bytes()).await.unwrap();
    }
}
