use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum SqliteMessage {
    New { db: String },
    Write { db: String, statement: String },
    Read { db: String, query: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SqlValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

pub trait Deserializable: for<'de> Deserialize<'de> + Sized {
    fn from_serialized(bytes: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(bytes)
    }
}

impl Deserializable for Vec<SqlValue> {}
impl Deserializable for Vec<Vec<SqlValue>> {}


#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub enum SqliteError {
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
