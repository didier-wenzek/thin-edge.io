use crate::Message;
use std::io::Error;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum RuntimeError {
    #[error("Configuration error")]
    ConfigError,

    #[error("I/O error")]
    IoError,

    #[error("Send failed: the channel is closed")]
    SendError(#[from] futures::channel::mpsc::SendError),
}

impl From<futures::io::Error> for RuntimeError {
    fn from(_: Error) -> Self {
        RuntimeError::IoError
    }
}

impl Message for RuntimeError {}