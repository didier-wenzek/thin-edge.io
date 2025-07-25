mod partial_response;
use crate::error::DownloadError;
use crate::error::ErrContext;
use anyhow::anyhow;
use backoff::future::retry_notify;
use backoff::ExponentialBackoff;
use certificate::CloudHttpConfig;
use log::debug;
use log::info;
use log::warn;
use nix::sys::statvfs;
pub use partial_response::InvalidResponseError;
use reqwest::header;
use reqwest::header::HeaderMap;
use reqwest::Client;
use reqwest::Identity;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::fs::File;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use tedge_utils::file::FileError;

#[cfg(target_os = "linux")]
use nix::fcntl::fallocate;
#[cfg(target_os = "linux")]
use nix::fcntl::FallocateFlags;
#[cfg(target_os = "linux")]
use std::os::unix::prelude::AsRawFd;

fn default_backoff() -> ExponentialBackoff {
    // Default retry is an exponential retry with a limit of 15 minutes total.
    // Let's set some more reasonable retry policy so we don't block the downloads for too long.
    ExponentialBackoff {
        initial_interval: Duration::from_secs(15),
        max_elapsed_time: Some(Duration::from_secs(300)),
        randomization_factor: 0.1,
        ..Default::default()
    }
}

/// Describes a request used to retrieve the file.
#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct DownloadInfo {
    pub url: String,
    #[serde(skip)]
    pub headers: HeaderMap,
}

impl From<&str> for DownloadInfo {
    fn from(url: &str) -> Self {
        Self::new(url)
    }
}

impl DownloadInfo {
    /// Creates new [`DownloadInfo`] from a URL.
    pub fn new(url: &str) -> Self {
        Self {
            url: url.into(),
            headers: HeaderMap::new(),
        }
    }

    /// Creates new [`DownloadInfo`] from a URL with authentication.
    pub fn with_headers(self, header_map: HeaderMap) -> Self {
        Self {
            headers: header_map,
            ..self
        }
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub fn is_empty(&self) -> bool {
        self.url.trim().is_empty()
    }
}

/// A struct which manages file downloads.
#[derive(Debug)]
pub struct Downloader {
    target_filename: PathBuf,
    backoff: ExponentialBackoff,
    client: Client,
}

impl Downloader {
    /// Creates a new downloader which downloads to a target directory and uses
    /// default permissions.
    pub fn new(
        target_path: PathBuf,
        identity: Option<Identity>,
        cloud_http_config: CloudHttpConfig,
    ) -> Self {
        let mut client_builder = cloud_http_config.client_builder();
        if let Some(identity) = identity {
            client_builder = client_builder.identity(identity);
        }
        let client = client_builder.build().expect("Client builder is valid");
        Self {
            target_filename: target_path,
            backoff: default_backoff(),
            client,
        }
    }

    pub fn set_backoff(&mut self, backoff: ExponentialBackoff) {
        self.backoff = backoff;
    }

    /// Downloads a file using an exponential backoff strategy.
    ///
    /// Partial backoff has a minimal interval of 30s and max elapsed time of
    /// 5min. It applies only when sending a request and either receiving a
    /// 500-599 response status or when request couldn't be made due to some
    /// network-related failure. If a network failure happens when downloading
    /// response body chunks, in some cases it doesn't trigger any errors, but
    /// just grinds down to a halt, e.g. when disconnecting from a network.
    ///
    /// To learn more about the backoff, see documentation of the
    /// [`backoff`](backoff) crate.
    ///
    /// Requests partial ranges if a transient error happened while downloading
    /// and the server response included `Accept-Ranges` header.
    pub async fn download(&self, url: &DownloadInfo) -> Result<(), DownloadError> {
        let tmp_target_path = self.temp_filename().await?;
        let target_file_path = self.target_filename.as_path();

        let temp_dir = self
            .target_filename
            .parent()
            .unwrap_or(&self.target_filename);

        let mut file = tempfile::NamedTempFile::new_in(temp_dir)
            .context("Could not write to temporary file".to_string())?;

        let mut response = self.request_range_from(url, 0).await?;

        let file_len = response.content_length().unwrap_or(0);
        info!(
            "Downloading file from url={url:?}, len={file_len}",
            url = url.url
        );

        if file_len > 0 {
            try_pre_allocate_space(file.as_file(), &tmp_target_path, file_len)?;
            debug!("preallocated space for file {tmp_target_path:?}, len={file_len}");
        }

        if let Err(err) = save_chunks_to_file_at(&mut response, file.as_file_mut(), 0).await {
            match err {
                SaveChunksError::Network(err) => {
                    warn!("Error while downloading response: {err}.\nRetrying...");

                    match response.headers().get(header::ACCEPT_RANGES) {
                        Some(unit) if unit == "bytes" => {
                            self.download_remaining(url, file.as_file_mut()).await?;
                        }
                        _ => {
                            self.retry(url, file.as_file_mut()).await?;
                        }
                    }
                }
                SaveChunksError::Io(err) => {
                    return Err(DownloadError::FromIo {
                        source: err,
                        context: "Error while saving to file".to_string(),
                    })
                }
            }
        }

        // Move the downloaded file to the final destination
        debug!(
            "Moving downloaded file from {:?} to {:?}",
            &tmp_target_path, &target_file_path
        );

        file.persist(target_file_path)
            .map_err(|p| p.error)
            .context("Could not persist temporary file".to_string())?;

        Ok(())
    }

    /// Retries the download requesting only the remaining file part.
    ///
    /// If the server does support it, a range request is used to download only
    /// the remaining range of the file. If the range request could not be used,
    /// [`retry`](Downloader::retry) is used instead.
    async fn download_remaining(
        &self,
        url: &DownloadInfo,
        file: &mut File,
    ) -> Result<(), DownloadError> {
        loop {
            let file_pos = file
                .stream_position()
                .context("Can't get file cursor position".to_string())?;

            let mut response = self.request_range_from(url, file_pos).await?;
            let offset = partial_response::response_range_start(&response)?;

            if offset != 0 {
                info!("Resuming file download at position={file_pos}");
            } else {
                info!("Could not resume download, restarting");
            }

            match save_chunks_to_file_at(&mut response, file, offset).await {
                Ok(()) => break,

                Err(SaveChunksError::Network(err)) => {
                    warn!("Error while downloading response: {err}.\nRetrying...");
                    continue;
                }

                Err(SaveChunksError::Io(err)) => {
                    return Err(DownloadError::FromIo {
                        source: err,
                        context: "Error while saving to file".to_string(),
                    })
                }
            }
        }

        Ok(())
    }

    /// Retries downloading the file.
    ///
    /// Retries initial request and downloads the entire file once again. If
    /// upon the initial request server signaled support for range requests,
    /// [`download_remaining`](Downloader::download_remaining) is used instead.
    async fn retry(&self, url: &DownloadInfo, file: &mut File) -> Result<(), DownloadError> {
        loop {
            info!("Could not resume download, restarting");
            let mut response = self.request_range_from(url, 0).await?;

            match save_chunks_to_file_at(&mut response, file, 0).await {
                Ok(()) => break,

                Err(SaveChunksError::Network(err)) => {
                    warn!("Error while downloading response: {err}.\nRetrying...");
                    continue;
                }

                Err(SaveChunksError::Io(err)) => {
                    return Err(DownloadError::FromIo {
                        source: err,
                        context: "Error while saving to file".to_string(),
                    })
                }
            }
        }

        Ok(())
    }

    /// Returns the filename.
    pub fn filename(&self) -> &Path {
        self.target_filename.as_path()
    }

    /// Builds a temporary filename the file will be downloaded into.
    async fn temp_filename(&self) -> Result<PathBuf, DownloadError> {
        if self.target_filename.is_relative() {
            return Err(FileError::InvalidFileName {
                path: self.target_filename.clone(),
                source: anyhow!("Path can't be relative"),
            })?;
        }

        if self.target_filename.exists() {
            // Confirm that the file has write access before any http request attempt
            self.has_write_access()?;
        } else if let Some(file_parent) = self.target_filename.parent() {
            if !file_parent.exists() {
                tokio::fs::create_dir_all(file_parent)
                    .await
                    .context(format!(
                        "error creating parent directories for {file_parent:?}"
                    ))?;
            }
        }

        // Download file to the target directory with a temp name
        let target_file_path = &self.target_filename;
        let file_name = target_file_path
            .file_name()
            .ok_or_else(|| FileError::InvalidFileName {
                path: target_file_path.clone(),
                source: anyhow!("Does not name a valid file"),
            })?
            .to_str()
            .ok_or_else(|| FileError::InvalidFileName {
                path: target_file_path.clone(),
                source: anyhow!("Path is not valid unicode"),
            })?;
        let parent_dir = target_file_path
            .parent()
            .ok_or_else(|| FileError::InvalidFileName {
                path: target_file_path.clone(),
                source: anyhow!("Does not name a valid file"),
            })?;

        let tmp_file_name = format!("{file_name}.tmp");
        Ok(parent_dir.join(tmp_file_name))
    }

    fn has_write_access(&self) -> Result<(), DownloadError> {
        let metadata = if self.target_filename.is_file() {
            let target_filename = &self.target_filename;
            fs::metadata(target_filename)
                .context(format!("error getting metadata of {target_filename:?}"))?
        } else {
            // If the file does not exist before downloading file, check the directory perms
            let parent_dir =
                &self
                    .target_filename
                    .parent()
                    .ok_or_else(|| DownloadError::NoWriteAccess {
                        path: self.target_filename.clone(),
                    })?;
            fs::metadata(parent_dir).context(format!("error getting metadata of {parent_dir:?}"))?
        };

        // Write permission check
        if metadata.permissions().readonly() {
            Err(DownloadError::NoWriteAccess {
                path: self.target_filename.clone(),
            })
        } else {
            Ok(())
        }
    }

    /// Deletes the file if it was downloaded.
    pub async fn cleanup(&self) -> Result<(), DownloadError> {
        let _res = tokio::fs::remove_file(&self.target_filename).await;
        Ok(())
    }

    /// Requests either the entire HTTP resource, or its part, from an offset to the
    /// end.
    ///
    /// If `range_start` is `0`, then a regular GET request is sent. Otherwise, a
    /// request for a range of the resource, starting from `range_start`, until EOF,
    /// is sent.
    ///
    /// We use a half-open range with only a lower bound, because we expect to use
    /// it to download static resources which do not change, and only as a recovery
    /// mechanism in case of network failures.
    async fn request_range_from(
        &self,
        url: &DownloadInfo,
        range_start: u64,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let backoff = self.backoff.clone();

        let operation = || async {
            let mut request = self.client.get(url.url());
            request = request.headers(url.headers.clone());

            if range_start != 0 {
                request = request.header("Range", format!("bytes={range_start}-"));
            }

            request
                .send()
                .await
                .and_then(|response| response.error_for_status())
                .map_err(reqwest_err_to_backoff)
        };

        retry_notify(backoff, operation, |err, dur: Duration| {
            let dur = dur.as_secs();
            warn!("Temporary failure: {err}. Retrying in {dur}s",)
        })
        .await
    }
}

/// Decides whether HTTP request error is retryable.
fn reqwest_err_to_backoff(err: reqwest::Error) -> backoff::Error<reqwest::Error> {
    if err.is_timeout() || err.is_connect() {
        return backoff::Error::transient(err);
    }
    if let Some(status) = err.status() {
        if status.is_server_error() {
            return backoff::Error::transient(err);
        }
    }
    backoff::Error::permanent(err)
}

/// Saves a response body chunks starting from an offset.
async fn save_chunks_to_file_at(
    response: &mut reqwest::Response,
    writer: &mut File,
    offset: u64,
) -> Result<(), SaveChunksError> {
    writer.seek(SeekFrom::Start(offset))?;

    while let Some(bytes) = response.chunk().await? {
        writer.write_all(&bytes)?;
    }
    writer.flush()?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
enum SaveChunksError {
    #[error("Error reading from network")]
    Network(#[from] reqwest::Error),

    #[error("Unable to write data to the file")]
    Io(#[from] std::io::Error),
}

#[allow(clippy::unnecessary_cast)]
fn try_pre_allocate_space(file: &File, path: &Path, file_len: u64) -> Result<(), DownloadError> {
    if file_len == 0 {
        return Ok(());
    }

    let tmpstats =
        statvfs::fstatvfs(file).context(format!("Can't stat file descriptor for file {path:?}"))?;

    // Reserve 5% of total disk space
    let five_percent_disk_space =
        (tmpstats.blocks() as i64 * tmpstats.block_size() as i64) * 5 / 100;
    let usable_disk_space =
        tmpstats.blocks_free() as i64 * tmpstats.block_size() as i64 - five_percent_disk_space;

    if file_len >= usable_disk_space.max(0) as u64 {
        return Err(DownloadError::InsufficientSpace);
    }

    // Reserve diskspace
    #[cfg(target_os = "linux")]
    let _ = fallocate(
        file.as_raw_fd(),
        FallocateFlags::empty(),
        0,
        file_len.try_into().expect("file too large to fit in i64"),
    );

    Ok(())
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use axum::Router;
    use hyper::header::AUTHORIZATION;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::PrivateKeyDer;
    use rustls::RootCertStore;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tempfile::NamedTempFile;
    use tempfile::TempDir;
    use test_case::test_case;
    use tokio::io::AsyncBufReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::io::BufReader;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn downloader_download_content_no_auth() {
        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_status(200)
            .with_body(b"hello")
            .create_async()
            .await;

        let target_dir_path = TempDir::new().unwrap();
        let target_path = target_dir_path.path().join("test_download");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let mut downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        downloader.set_backoff(ExponentialBackoff {
            current_interval: Duration::ZERO,
            ..Default::default()
        });
        downloader.download(&url).await.unwrap();

        let log_content = std::fs::read(downloader.filename()).unwrap();

        assert_eq!("hello".as_bytes(), log_content);
    }

    #[tokio::test]
    async fn downloader_download_to_target_path() {
        let temp_dir = tempdir().unwrap();

        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_status(200)
            .with_body(b"hello")
            .create_async()
            .await;

        let target_path = temp_dir.path().join("downloaded_file.txt");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let downloader = Downloader::new(target_path.clone(), None, CloudHttpConfig::test_value());
        downloader.download(&url).await.unwrap();

        let file_content = std::fs::read(target_path).unwrap();

        assert_eq!(file_content, "hello".as_bytes());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "Overriding Content-Length doesn't work in mockito"]
    async fn downloader_download_with_content_length_larger_than_usable_disk_space() {
        use nix::sys::statvfs;
        let tmpstats = statvfs::statvfs("/tmp").unwrap();
        let usable_disk_space = (tmpstats.blocks_free() as u64) * (tmpstats.block_size() as u64);

        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_header("content-length", &usable_disk_space.to_string())
            .create_async()
            .await;

        let target_dir_path = TempDir::new().unwrap();
        let target_path = target_dir_path.path().join("test_download_with_length");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        let err = downloader.download(&url).await.unwrap_err();
        assert!(matches!(err, DownloadError::InsufficientSpace));
    }

    #[tokio::test]
    async fn returns_proper_errors_for_invalid_filenames() {
        let temp_dir = tempdir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_status(200)
            .with_body(b"hello")
            .create_async()
            .await;

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        // empty filename
        let downloader = Downloader::new("".into(), None, CloudHttpConfig::test_value());
        let err = downloader.download(&url).await.unwrap_err();
        assert!(matches!(
            err,
            DownloadError::FromFileError(FileError::InvalidFileName { .. })
        ));

        // invalid unicode filename
        let path = unsafe { String::from_utf8_unchecked(b"\xff".to_vec()) };
        let downloader = Downloader::new(path.into(), None, CloudHttpConfig::test_value());
        let err = downloader.download(&url).await.unwrap_err();
        assert!(matches!(
            err,
            DownloadError::FromFileError(FileError::InvalidFileName { .. })
        ));

        // relative path filename
        let downloader = Downloader::new("myfile.txt".into(), None, CloudHttpConfig::test_value());
        let err = downloader.download(&url).await.unwrap_err();
        assert!(matches!(
            err,
            DownloadError::FromFileError(FileError::InvalidFileName { .. })
        ));
        println!("{err:?}", err = anyhow::Error::from(err));
    }

    #[tokio::test]
    async fn writing_to_existing_file() {
        let temp_dir = tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_status(200)
            .with_body(b"hello")
            .create_async()
            .await;

        let target_file_path = temp_dir.path().join("downloaded_file.txt");
        std::fs::File::create(&target_file_path).unwrap();

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let downloader = Downloader::new(
            target_file_path.clone(),
            None,
            CloudHttpConfig::test_value(),
        );
        downloader.download(&url).await.unwrap();

        let file_content = std::fs::read(target_file_path).unwrap();

        assert_eq!(file_content, "hello".as_bytes());
    }

    #[tokio::test]
    async fn downloader_download_with_reasonable_content_length() {
        let file = create_file_with_size(10 * 1024 * 1024).unwrap();
        let file_path = file.into_temp_path();

        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_body_from_file(&file_path)
            .create_async()
            .await;

        let target_dir_path = TempDir::new().unwrap();
        let target_path = target_dir_path.path().join("test_download_with_length");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());

        downloader.download(&url).await.unwrap();

        let log_content = std::fs::read(downloader.filename()).unwrap();
        let expected_content = std::fs::read(file_path).unwrap();
        assert_eq!(log_content, expected_content);
    }

    #[tokio::test]
    async fn downloader_download_verify_file_content() {
        let file = create_file_with_size(10).unwrap();

        let mut server = mockito::Server::new_async().await;
        let _mock1 = server
            .mock("GET", "/some_file.txt")
            .with_body_from_file(file.into_temp_path())
            .create_async()
            .await;

        let target_dir_path = TempDir::new().unwrap();
        let target_path = target_dir_path.path().join("test_download_with_length");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        downloader.download(&url).await.unwrap();

        let log_content = std::fs::read(downloader.filename()).unwrap();

        assert_eq!("Some data!".as_bytes(), log_content);
    }

    #[tokio::test]
    async fn downloader_download_without_content_length() {
        let mut server = mockito::Server::new_async().await;
        let _mock1 = server.mock("GET", "/some_file.txt").create_async().await;

        let target_dir_path = TempDir::new().unwrap();
        let target_path = target_dir_path.path().join("test_download_without_length");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        downloader.download(&url).await.unwrap();

        assert_eq!("".as_bytes(), std::fs::read(downloader.filename()).unwrap());
    }

    #[tokio::test]
    async fn doesnt_leave_tmpfiles_on_errors() {
        let server = mockito::Server::new_async().await;

        let target_dir_path = TempDir::new().unwrap();
        let target_path = target_dir_path.path().join("test_doesnt_leave_tmpfiles");

        let mut target_url = server.url();
        target_url.push_str("/some_file.txt");

        let url = DownloadInfo::new(&target_url);

        let mut downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        downloader.set_backoff(ExponentialBackoff {
            current_interval: Duration::ZERO,
            max_interval: Duration::ZERO,
            max_elapsed_time: Some(Duration::ZERO),
            ..Default::default()
        });
        downloader.download(&url).await.unwrap_err();

        assert_eq!(fs::read_dir(target_dir_path.path()).unwrap().count(), 0);
    }

    /// This test simulates HTTP response where a connection just drops and a
    /// client hits a timeout, having downloaded only part of the response.
    ///
    /// I couldn't find a reliable way to drop the TCP connection without doing
    /// a closing handshake, so the TCP connection is closed normally, but
    /// because `Transfer-Encoding: chunked` is used, when closing the
    /// connection, the client sees that it hasn't received a 0-length
    /// termination chunk (which signals that the entire HTTP chunked body has
    /// been sent) and retries the request with a `Range` header.
    #[tokio::test]
    async fn resume_download_when_disconnected() {
        let chunk_size = 4;
        let file = "AAAABBBBCCCCDDDD";

        let listener = TcpListener::bind("localhost:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_task = tokio::spawn(async move {
            while let Ok((mut stream, _addr)) = listener.accept().await {
                let response_task = async move {
                    let (reader, mut writer) = stream.split();
                    let mut lines = BufReader::new(reader).lines();
                    let mut range: Option<std::ops::Range<usize>> = None;

                    // We got an HTTP request, read the lines of the request
                    'inner: while let Ok(Some(line)) = lines.next_line().await {
                        if line.to_ascii_lowercase().contains("range:") {
                            let (_, bytes) = line.split_once('=').unwrap();
                            let (start, end) = bytes.split_once('-').unwrap();
                            let start = start.parse().unwrap_or(0);
                            let end = end.parse().unwrap_or(file.len());
                            range = Some(start..end)
                        }
                        // On `\r\n\r\n` (empty line) stop reading the request
                        // and start responding
                        if line.is_empty() {
                            break 'inner;
                        }
                    }

                    if let Some(range) = range {
                        let start = range.start;
                        let end = range.end;
                        let header = format!(
                            "HTTP/1.1 206 Partial Content\r\n\
                            transfer-encoding: chunked\r\n\
                            connection: close\r\n\
                            content-type: application/octet-stream\r\n\
                            content-range: bytes {start}-{end}/*\r\n\
                            accept-ranges: bytes\r\n"
                        );
                        // answer with range starting 1 byte before what client
                        // requested to ensure it correctly parses content-range
                        // and doesn't just keep writing to where it left off in
                        // the previous request
                        let next = (start - 1 + chunk_size).min(file.len());
                        let body = &file[start..next];

                        let size = body.len();
                        let msg = format!("{header}\r\n{size}\r\n{body}\r\n");
                        debug!("sending message = {msg}");
                        writer.write_all(msg.as_bytes()).await.unwrap();
                        writer.flush().await.unwrap();
                    } else {
                        let header = "\
                            HTTP/1.1 200 OK\r\n\
                            transfer-encoding: chunked\r\n\
                            connection: close\r\n\
                            content-type: application/octet-stream\r\n\
                            accept-ranges: bytes\r\n";

                        let body = "AAAA";
                        let msg = format!("{header}\r\n4\r\n{body}\r\n");
                        writer.write_all(msg.as_bytes()).await.unwrap();
                        writer.flush().await.unwrap();
                    }
                };
                tokio::spawn(response_task);
            }
        });

        // Wait until task binds a listener on the TCP port
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let tmpdir = TempDir::new().unwrap();
        let target_path = tmpdir.path().join("partial_download");

        let downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        let url = DownloadInfo::new(&format!("http://localhost:{port}/"));

        downloader.download(&url).await.unwrap();
        let saved_file = std::fs::read_to_string(downloader.filename()).unwrap();
        assert_eq!(saved_file, file);

        downloader.cleanup().await.unwrap();

        server_task.abort();
    }

    // Parameters:
    //
    // - status code
    // - bearer token boolean
    // - maybe url
    // - expected std error
    // - description
    #[test_case(
        200,
        false,
        Some("not_a_url"),
        "URL"
        ; "builder error"
    )]
    #[test_case(
        200,
        true,
        Some("not_a_url"),
        "URL"
        ; "builder error with auth"
    )]
    #[test_case(
        200,
        false,
        Some("http://not_a_url"),
        "dns error"
        ; "dns error"
    )]
    #[test_case(
        200,
        true,
        Some("http://not_a_url"),
        "dns error"
        ; "dns error with auth"
    )]
    #[test_case(
        404,
        false,
        None,
        "404 Not Found"
        ; "client error"
    )]
    #[test_case(
        404,
        true,
        None,
        "404 Not Found"
        ; "client error with auth"
    )]
    #[tokio::test]
    async fn downloader_download_processing_error(
        status_code: usize,
        with_token: bool,
        url: Option<&str>,
        expected_err: &str,
    ) {
        let target_dir_path = TempDir::new().unwrap();
        let mut server = mockito::Server::new_async().await;

        // bearer/no bearer setup
        let _mock1 = {
            if with_token {
                server
                    .mock("GET", "/some_file.txt")
                    .match_header("authorization", "Bearer token")
                    .with_status(status_code)
                    .create_async()
                    .await
            } else {
                server
                    .mock("GET", "/some_file.txt")
                    .with_status(status_code)
                    .create_async()
                    .await
            }
        };

        // url/no url setup
        let url = {
            if let Some(url) = url {
                DownloadInfo::new(url)
            } else {
                let mut target_url = server.url();
                target_url.push_str("/some_file.txt");
                DownloadInfo::new(&target_url)
            }
        };

        // applying http auth header
        let url = {
            if with_token {
                let mut headers = HeaderMap::new();
                headers.append(AUTHORIZATION, "Bearer token".parse().unwrap());
                url.with_headers(headers)
            } else {
                url
            }
        };

        let target_path = target_dir_path.path().join("test_download");
        let mut downloader = Downloader::new(target_path, None, CloudHttpConfig::test_value());
        downloader.set_backoff(ExponentialBackoff {
            max_elapsed_time: Some(Duration::ZERO),
            ..Default::default()
        });
        match downloader.download(&url).await {
            Ok(_success) => panic!("Expected client error."),
            Err(err) => {
                // `Error::to_string` uses a Display trait and only contains a
                // top-level error message, and not any lower level contexts. To
                // make sure that we look at the entire error chain, we wrap the
                // error in `anyhow::Error` which reports errors by printing the
                // entire error chain. We can then check keywords that we want
                // appear somewhere in the error chain

                let err = anyhow::Error::from(err);
                println!("{err:?}");

                // We use debug representation because that's what anyhow uses
                // to pretty print error report chain
                assert!(format!("{err:?}")
                    .to_ascii_lowercase()
                    .contains(&expected_err.to_ascii_lowercase()));
            }
        };
    }

    #[tokio::test]
    async fn downloader_error_shows_certificate_required_error_when_appropriate() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_cert = rcgen::generate_simple_self_signed(["localhost".into()]).unwrap();
        let cert = server_cert.cert.der().clone();
        let key = PrivateKeyDer::from_pem_slice(server_cert.signing_key.serialize_pem().as_bytes())
            .unwrap();
        let mut accepted_certs = RootCertStore::empty();
        accepted_certs.add(cert.clone()).unwrap();
        let config = axum_tls::ssl_config(vec![cert.clone()], key, Some(accepted_certs)).unwrap();
        let app = Router::new();

        tokio::spawn(axum_tls::start_tls_server(
            listener.into_std().unwrap(),
            config,
            app,
        ));

        let req_cert = reqwest::Certificate::from_der(&cert).unwrap();
        let url = DownloadInfo::new(&format!("http://localhost:{port}"));

        let downloader = Downloader::new(
            PathBuf::from("/tmp/should-never-exist"),
            None,
            CloudHttpConfig::new(Arc::from(vec![req_cert]), None),
        );
        let err = downloader.download(&url).await.unwrap_err();
        let err = anyhow::Error::new(err);

        assert!(dbg!(format!("{err:#}")).contains("received fatal alert: CertificateRequired"));
    }

    fn create_file_with_size(size: usize) -> Result<NamedTempFile, anyhow::Error> {
        let mut file = NamedTempFile::new().unwrap();
        let data: String = "Some data!".into();
        let loops = size / data.len();
        let mut buffer = String::with_capacity(size);
        for _ in 0..loops {
            buffer.push_str("Some data!");
        }

        file.write_all(buffer.as_bytes()).unwrap();
        file.flush().unwrap();

        Ok(file)
    }
}
