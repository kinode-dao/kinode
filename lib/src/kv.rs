use crate::types::core::{CapMessage, PackageId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Actions are sent to a specific key value database, `db` is the name,
/// `package_id` is the [`PackageId`]. Capabilities are checked, you can access another process's
/// database if it has given you the [`crate::Capability`].
#[derive(Debug, Serialize, Deserialize)]
pub struct KvRequest {
    pub package_id: PackageId,
    pub db: String,
    pub action: KvAction,
}

/// IPC Action format, representing operations that can be performed on the key-value runtime module.
/// These actions are included in a KvRequest sent to the kv:distro:sys runtime module.
#[derive(Debug, Serialize, Deserialize, Clone)]
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
    Get { key: Vec<u8> },
    /// Begins a new transaction for atomic operations.
    BeginTx,
    /// Commits all operations in the specified transaction.
    ///
    /// # Parameters
    /// * `tx_id` - The ID of the transaction to commit
    Commit { tx_id: u64 },
    /// Creates a backup of the database.
    Backup,
    /// Starts an iterator over the database contents.
    ///
    /// # Parameters
    /// * `prefix` - Optional byte vector to filter keys by prefix
    IterStart { prefix: Option<Vec<u8>> },
    /// Advances the iterator and returns the next batch of items.
    ///
    /// # Parameters
    /// * `iterator_id` - The ID of the iterator to advance
    /// * `count` - Maximum number of items to return
    IterNext { iterator_id: u64, count: u64 },
    /// Closes an active iterator.
    ///
    /// # Parameters
    /// * `iterator_id` - The ID of the iterator to close
    IterClose { iterator_id: u64 },
}

/// Response types for key-value store operations.
/// These responses are returned after processing a KvAction request.
#[derive(Debug, Serialize, Deserialize)]
pub enum KvResponse {
    /// Indicates successful completion of an operation.
    Ok,
    /// Returns the transaction ID for a newly created transaction.
    ///
    /// # Fields
    /// * `tx_id` - The ID of the newly created transaction
    BeginTx { tx_id: u64 },
    /// Returns the key that was retrieved from the database.
    ///
    /// # Fields
    /// * `key` - The retrieved key as a byte vector
    Get { key: Vec<u8> },
    /// Indicates an error occurred during the operation.
    ///
    /// # Fields
    /// * `error` - The specific error that occurred
    Err { error: KvError },
    /// Returns the ID of a newly created iterator.
    ///
    /// # Fields
    /// * `iterator_id` - The ID of the created iterator
    IterStart { iterator_id: u64 },
    /// Indicates whether the iterator has more items.
    ///
    /// # Fields
    /// * `done` - True if there are no more items to iterate over
    IterNext { done: bool },
    /// Confirms the closure of an iterator.
    ///
    /// # Fields
    /// * `iterator_id` - The ID of the closed iterator
    IterClose { iterator_id: u64 },
}

/// Errors that can occur during key-value store operations.
/// These errors are returned as part of `KvResponse::Err` when an operation fails.
#[derive(Debug, Serialize, Deserialize, Error)]
pub enum KvError {
    /// The requested database does not exist.
    #[error("Database does not exist")]
    NoDb,

    /// The requested key was not found in the database.
    #[error("Key not found in database")]
    KeyNotFound,

    /// No active transaction found for the given transaction ID.
    #[error("Transaction not found")]
    NoTx,

    /// The specified iterator was not found.
    #[error("Iterator not found")]
    NoIterator,

    /// The operation requires capabilities that the caller doesn't have.
    ///
    /// # Fields
    /// * `error` - Description of the missing capability or permission
    #[error("Missing required capability: {error}")]
    NoCap { error: String },

    /// An internal RocksDB error occurred during the operation.
    ///
    /// # Fields
    /// * `action` - The operation that was being performed
    /// * `error` - The specific error message from RocksDB
    #[error("RocksDB error during {action}: {error}")]
    RocksDBError { action: String, error: String },

    /// Error parsing or processing input data.
    ///
    /// # Fields
    /// * `error` - Description of what was invalid about the input
    #[error("Invalid input: {error}")]
    InputError { error: String },

    /// An I/O error occurred during the operation.
    ///
    /// # Fields
    /// * `error` - Description of the I/O error
    #[error("I/O error: {error}")]
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
