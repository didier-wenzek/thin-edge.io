use std::path::Path;

pub async fn start_watchdog(_config_dir: &Path) -> Result<(), anyhow::Error> {
    Err(anyhow::Error::from(WatchdogError::WatchdogNotAvailable))
}

#[derive(Debug, thiserror::Error)]
pub enum WatchdogError {
    #[error("The watchdog is not available on this platform")]
    WatchdogNotAvailable,
}
