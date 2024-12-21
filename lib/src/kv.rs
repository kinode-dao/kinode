use crate::types::core::PackageId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// IPC Request format for the kv:distro:sys runtime module.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KvRequest {
    pub package_id: PackageId,
    pub db: String,
    pub action: KvAction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KvAction {
    Open,
    RemoveDb,
    Set { key: Vec<u8>, tx_id: Option<u64> },
    Delete { key: Vec<u8>, tx_id: Option<u64> },
    Get(Vec<u8>),
    BeginTx,
    Commit { tx_id: u64 },
    Backup,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KvResponse {
    Ok,
    BeginTx { tx_id: u64 },
    Get(Vec<u8>),
    Err(KvError),
}

#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum KvError {
    #[error("db [{0}, {1}] does not exist")]
    NoDb(PackageId, String),
    #[error("key not found")]
    KeyNotFound,
    #[error("no transaction {0} found")]
    NoTx(u64),
    #[error("no write capability for requested DB")]
    NoWriteCap,
    #[error("no read capability for requested DB")]
    NoReadCap,
    #[error("request to open or remove DB with mismatching package ID")]
    MismatchingPackageId,
    #[error("failed to generate capability for new DB")]
    AddCapFailed,
    #[error("kv got a malformed request that either failed to deserialize or was missing a required blob")]
    MalformedRequest,
    #[error("RocksDB internal error: {0}")]
    RocksDBError(String),
    #[error("IO error: {0}")]
    IOError(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KvCapabilityParams {
    pub kind: KvCapabilityKind,
    pub db_key: (PackageId, String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KvCapabilityKind {
    Read,
    Write,
}

impl std::fmt::Display for KvAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<std::io::Error> for KvError {
    fn from(err: std::io::Error) -> Self {
        KvError::IOError(err.to_string())
    }
}
