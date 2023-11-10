use crate::types::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// The structure of an IPC message sent to the HTTP server. Should be
/// serialized as JSON bytes. HTTP server will respond with `HttpServerResponse`,
/// which will also be serialized as JSON bytes.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpServerMessage {
    BindPath {
        path: String,
        authenticated: bool,
        local_only: bool,
    },
    WebSocketPush(WebSocketPush),
    ServerAction(ServerAction),
    WsRegister(WsRegister),                 // Coming from a proxy
    WsProxyDisconnect(WsProxyDisconnect),   // Coming from a proxy
    WsMessage(WsMessage),                   // Coming from a proxy
    EncryptedWsMessage(EncryptedWsMessage), // Coming from a proxy
}

/// Any Request sent to http_server, if it expects a response, will be given this
/// response serialized to JSON bytes.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpServerResponse {
    Ok(()),
    Error(HttpServerError),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>, // TODO does this use a lot of memory?
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpServerError {
    #[error("http_server: json is None")]
    NoJson,
    #[error("http_server: response not ok")]
    ResponseError,
    #[error("http_server: bytes are None")]
    NoBytes,
    #[error(
        "http_server: JSON payload could not be parsed to HttpClientRequest: {error}. Got {:?}.",
        json
    )]
    BadJson { json: String, error: String },
    #[error("http_server: path binding error:  {:?}", error)]
    PathBind { error: String },
}

#[allow(dead_code)]
impl HttpServerError {
    pub fn kind(&self) -> &str {
        match *self {
            HttpServerError::NoJson { .. } => "NoJson",
            HttpServerError::NoBytes { .. } => "NoBytes",
            HttpServerError::BadJson { .. } => "BadJson",
            HttpServerError::ResponseError { .. } => "ResponseError",
            HttpServerError::PathBind { .. } => "PathBind",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub username: String,
    pub expiration: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebSocketServerTarget {
    pub node: String,
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebSocketPush {
    pub target: WebSocketServerTarget,
    pub is_text: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerAction {
    pub action: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegister {
    pub ws_auth_token: String,
    pub auth_token: String,
    pub channel_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsProxyDisconnect {
    // Doesn't require auth because it's coming from the proxy
    pub channel_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsMessage {
    pub ws_auth_token: String,
    pub auth_token: String,
    pub channel_id: String,
    pub target: Address,
    pub json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedWsMessage {
    pub ws_auth_token: String,
    pub auth_token: String,
    pub channel_id: String,
    pub target: Address,
    pub encrypted: String, // Encrypted JSON as hex with the 32-byte authentication tag appended
    pub nonce: String,     // Hex of the 12-byte nonce
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WebSocketClientMessage {
    WsRegister(WsRegister),
    WsMessage(WsMessage),
    EncryptedWsMessage(EncryptedWsMessage),
}
