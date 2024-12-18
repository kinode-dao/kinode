use crate::types::core::{CapMessage, PackageId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// IPC Request format for the kv:distro:sys runtime module.
#[derive(Debug, Serialize, Deserialize)]
pub struct KvRequest {
    pub package_id: PackageId,
    pub db: String,
    pub action: KvAction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum KvAction {
    Open,
    RemoveDb,
    Set { key: Vec<u8>, tx_id: Option<u64> },
    Delete { key: Vec<u8>, tx_id: Option<u64> },
    Get { key: Vec<u8> },
    BeginTx,
    Commit { tx_id: u64 },
    Backup,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KvResponse {
    Ok,
    BeginTx { tx_id: u64 },
    Get { key: Vec<u8> },
    Err { error: KvError },
}

#[derive(Debug, Serialize, Deserialize, Error)]
pub enum KvError {
    #[error("DbDoesNotExist")]
    NoDb,
    #[error("KeyNotFound")]
    KeyNotFound,
    #[error("no Tx found")]
    NoTx,
    #[error("No capability: {error}")]
    NoCap { error: String },
    #[error("rocksdb internal error: {error}")]
    RocksDBError { action: String, error: String },
    #[error("input bytes/json/key error: {error}")]
    InputError { error: String },
    #[error("IO error: {error}")]
    IOError { error: String },
}

impl std::fmt::Display for KvAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for KvError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        KvError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for KvError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        KvError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<std::io::Error> for KvError {
    fn from(err: std::io::Error) -> Self {
        KvError::IOError {
            error: err.to_string(),
        }
    }
}
