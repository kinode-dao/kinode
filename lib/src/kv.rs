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
    /// Opens an existing key-value database or creates a new one if it doesn't exist.
    Open,
    /// Permanently deletes the entire key-value database.
    RemoveDb,
    /// Sets a value for the specified key in the database.
    ///
    /// # Parameters
    /// * `key` - The key as a byte vector
    /// * `tx_id` - Optional transaction ID if this operation is part of a transaction
    ///
    /// Blob: Value in Vec<u8>
    Set { key: Vec<u8>, tx_id: Option<u64> },
    /// Deletes a key-value pair from the database.
    ///
    /// # Parameters
    /// * `key` - The key to delete as a byte vector
    /// * `tx_id` - Optional transaction ID if this operation is part of a transaction
    Delete { key: Vec<u8>, tx_id: Option<u64> },
    /// Retrieves the value associated with the specified key.
    ///
    /// # Parameters
    /// * `key` - The key to look up as a byte vector
    Get(Vec<u8>),
    /// Begins a new transaction for atomic operations.
    BeginTx,
    /// Commits all operations in the specified transaction.
    ///
    /// # Parameters
    /// * `tx_id` - The ID of the transaction to commit
    Commit { tx_id: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum KvResponse {
    /// Indicates successful completion of an operation.
    /// Sent in response to actions Open, RemoveDb, Set, Delete, and Commit.
    Ok,
    /// Returns the transaction ID for a newly created transaction.
    ///
    /// # Fields
    /// * `tx_id` - The ID of the newly created transaction
    BeginTx { tx_id: u64 },
    /// Returns the value for the key that was retrieved from the database.
    ///
    /// # Fields
    /// * `key` - The retrieved key as a byte vector
    ///
    /// Blob: Value in Vec<u8>
    Get(Vec<u8>),
    /// Indicates an error occurred during the operation.
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
