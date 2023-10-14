use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum KeyValueMessage {
    New { db: String },
    Write { db: String, key: Vec<u8> },
    Read { db: String, key: Vec<u8> },
    Err { error: KeyValueError },
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub enum KeyValueError {
    #[error("DbDoesNotExist")]
    DbDoesNotExist,
    #[error("DbAlreadyExists")]
    DbAlreadyExists,
    #[error("NoCap")]
    NoCap,
    #[error("RejectForeign")]
    RejectForeign,
    #[error("UnexpectedResponse")]
    UnexpectedResponse,
}
