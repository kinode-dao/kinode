use crate::types::core::PackageId;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ValueRef};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Actions are sent to a specific SQLite database. `db` is the name,
/// `package_id` is the [`PackageId`] that created the database. Capabilities
/// are checked: you can access another process's database if it has given
/// you the read and/or write capability to do so.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SqliteRequest {
    pub package_id: PackageId,
    pub db: String,
    pub action: SqliteAction,
}

/// IPC Action format representing operations that can be performed on the
/// SQLite runtime module. These actions are included in a [`SqliteRequest`]
/// sent to the `sqlite:distro:sys` runtime module.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SqliteAction {
    /// Opens an existing key-value database or creates a new one if it doesn't exist.
    /// Requires `package_id` in [`SqliteRequest`] to match the package ID of the sender.
    /// The sender will own the database and can remove it with [`SqliteAction::RemoveDb`].
    ///
    /// A successful open will respond with [`SqliteResponse::Ok`]. Any error will be
    /// contained in the [`SqliteResponse::Err`] variant.
    Open,
    /// Permanently deletes the entire key-value database.
    /// Requires `package_id` in [`SqliteRequest`] to match the package ID of the sender.
    /// Only the owner can remove the database.
    ///
    /// A successful remove will respond with [`SqliteResponse::Ok`]. Any error will be
    /// contained in the [`SqliteResponse::Err`] variant.
    RemoveDb,
    /// Executes a write statement (INSERT/UPDATE/DELETE)
    ///
    /// * `statement` - SQL statement to execute
    /// * `tx_id` - Optional transaction ID
    /// * blob: Vec<SqlValue> - Parameters for the SQL statement, where SqlValue can be:
    ///   - null
    ///   - boolean
    ///   - i64
    ///   - f64
    ///   - String
    ///   - Vec<u8> (binary data)
    ///
    /// Using this action requires the sender to have the write capability
    /// for the database.
    ///
    /// A successful write will respond with [`SqliteResponse::Ok`]. Any error will be
    /// contained in the [`SqliteResponse::Err`] variant.
    Write {
        statement: String,
        tx_id: Option<u64>,
    },
    /// Executes a read query (SELECT)
    ///
    /// * blob: Vec<SqlValue> - Parameters for the SQL query, where SqlValue can be:
    ///   - null
    ///   - boolean
    ///   - i64
    ///   - f64
    ///   - String
    ///   - Vec<u8> (binary data)
    ///
    /// Using this action requires the sender to have the read capability
    /// for the database.
    ///
    /// A successful query will respond with [`SqliteResponse::Query`], where the
    /// response blob contains the results of the query. Any error will be contained
    /// in the [`SqliteResponse::Err`] variant.
    Query(String),
    /// Begins a new transaction for atomic operations.
    ///
    /// Sending this will prompt a [`SqliteResponse::BeginTx`] response with the
    /// transaction ID. Any error will be contained in the [`SqliteResponse::Err`] variant.
    BeginTx,
    /// Commits all operations in the specified transaction.
    ///
    /// # Parameters
    /// * `tx_id` - The ID of the transaction to commit
    ///
    /// A successful commit will respond with [`SqliteResponse::Ok`]. Any error will be
    /// contained in the [`SqliteResponse::Err`] variant.
    Commit { tx_id: u64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SqliteResponse {
    /// Indicates successful completion of an operation.
    /// Sent in response to actions Open, RemoveDb, Write, Query, BeginTx, and Commit.
    Ok,
    /// Returns the results of a query.
    ///
    /// * blob: Vec<Vec<SqlValue>> - Array of rows, where each row contains SqlValue types:
    ///   - null
    ///   - boolean
    ///   - i64
    ///   - f64
    ///   - String
    ///   - Vec<u8> (binary data)
    Read,
    /// Returns the transaction ID for a newly created transaction.
    ///
    /// # Fields
    /// * `tx_id` - The ID of the newly created transaction
    BeginTx { tx_id: u64 },
    /// Indicates an error occurred during the operation.
    Err(SqliteError),
}

/// Used in blobs to represent array row values in SQLite.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum SqlValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Boolean(bool),
    Null,
}

#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum SqliteError {
    #[error("db [{0}, {1}] does not exist")]
    NoDb(PackageId, String),
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
    #[error("write statement started with non-existent write keyword")]
    NotAWriteKeyword,
    #[error("read query started with non-existent read keyword")]
    NotAReadKeyword,
    #[error("parameters blob in read/write was misshapen or contained invalid JSON objects")]
    InvalidParameters,
    #[error("sqlite got a malformed request that failed to deserialize")]
    MalformedRequest,
    #[error("rusqlite error: {0}")]
    RusqliteError(String),
    #[error("IO error: {0}")]
    IOError(String),
}

/// The JSON parameters contained in all capabilities issued by `sqlite:distro:sys`.
///
/// # Fields
/// * `kind` - The kind of capability, either [`SqliteCapabilityKind::Read`] or [`SqliteCapabilityKind::Write`]
/// * `db_key` - The database key, a tuple of the [`PackageId`] that created the database and the database name
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SqliteCapabilityParams {
    pub kind: SqliteCapabilityKind,
    pub db_key: (PackageId, String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqliteCapabilityKind {
    Read,
    Write,
}

impl ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        match self {
            SqlValue::Integer(i) => i.to_sql(),
            SqlValue::Real(f) => f.to_sql(),
            SqlValue::Text(ref s) => s.to_sql(),
            SqlValue::Blob(ref b) => b.to_sql(),
            SqlValue::Boolean(b) => b.to_sql(),
            SqlValue::Null => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Null,
            )),
        }
    }
}

impl FromSql for SqlValue {
    fn column_result(value: ValueRef<'_>) -> Result<Self, FromSqlError> {
        match value {
            ValueRef::Integer(i) => Ok(SqlValue::Integer(i)),
            ValueRef::Real(f) => Ok(SqlValue::Real(f)),
            ValueRef::Text(t) => {
                let text_str = std::str::from_utf8(t).map_err(|_| FromSqlError::InvalidType)?;
                Ok(SqlValue::Text(text_str.to_string()))
            }
            ValueRef::Blob(b) => Ok(SqlValue::Blob(b.to_vec())),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl From<std::io::Error> for SqliteError {
    fn from(err: std::io::Error) -> Self {
        SqliteError::IOError(err.to_string())
    }
}

impl From<rusqlite::Error> for SqliteError {
    fn from(err: rusqlite::Error) -> Self {
        SqliteError::RusqliteError(err.to_string())
    }
}
