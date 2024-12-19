use crate::types::core::ProcessId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// IPC Requests for the state:distro:sys runtime module.
#[derive(Serialize, Deserialize, Debug)]
pub enum StateAction {
    GetState(ProcessId),
    SetState(ProcessId),
    DeleteState(ProcessId),
    Backup,
}

/// Responses for the state:distro:sys runtime module.
#[derive(Serialize, Deserialize, Debug)]
pub enum StateResponse {
    GetState,
    SetState,
    DeleteState,
    Backup,
    Err(StateError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum StateError {
    #[error("rocksdb internal error: {error}")]
    RocksDBError { action: String, error: String },
    #[error("startup error")]
    StartupError { action: String },
    #[error("bytes blob required for {action}")]
    BadBytes { action: String },
    #[error("bad request error: {error}")]
    BadRequest { error: String },
    #[error("Bad JSON blob: {error}")]
    BadJson { error: String },
    #[error("state not found for ProcessId {process_id}")]
    NotFound { process_id: ProcessId },
    #[error("IO error: {error}")]
    IOError { error: String },
}

impl StateError {
    pub fn kind(&self) -> &str {
        match *self {
            StateError::RocksDBError { .. } => "RocksDBError",
            StateError::StartupError { .. } => "StartupError",
            StateError::BadBytes { .. } => "BadBytes",
            StateError::BadRequest { .. } => "BadRequest",
            StateError::BadJson { .. } => "NoJson",
            StateError::NotFound { .. } => "NotFound",
            StateError::IOError { .. } => "IOError",
        }
    }
}

impl From<std::io::Error> for StateError {
    fn from(err: std::io::Error) -> Self {
        StateError::IOError {
            error: err.to_string(),
        }
    }
}
