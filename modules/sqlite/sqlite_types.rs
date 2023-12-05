use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum SqliteMessage {
    New { db: String },
    Write { db: String, statement: String, tx_id: Option<u64> },
    Read { db: String, query: String },
    Commit { db: String, tx_id: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SqlValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Boolean(bool),
    Null,
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub enum SqliteError {
    #[error("DbDoesNotExist")]
    DbDoesNotExist,
    #[error("DbAlreadyExists")]
    DbAlreadyExists,
    #[error("NoTx")]
    NoTx,
    #[error("NoCap")]
    NoCap,
    #[error("RejectForeign")]
    RejectForeign,
    #[error("UnexpectedResponse")]
    UnexpectedResponse,
    #[error("NotAWriteKeyword")]
    NotAWriteKeyword,
    #[error("NotAReadKeyword")]
    NotAReadKeyword,
    #[error("Invalid Parameters")]
    InvalidParameters,
}
