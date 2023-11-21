use crate::types::{Address, Payload};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// HTTP Request type that can be shared over WASM boundary to apps.
/// This is the one you receive from the http_server:sys:uqbar service.
#[derive(Debug, Serialize, Deserialize)]
pub struct IncomingHttpRequest {
    pub source_socket_addr: Option<String>, // will parse to SocketAddr
    pub method: String,                     // will parse to http::Method
    pub raw_path: String,
    pub headers: HashMap<String, String>,
    // BODY is stored in the payload, as bytes
}

/// HTTP Request type that can be shared over WASM boundary to apps.
/// This is the one you send to the http_client:sys:uqbar service.
#[derive(Debug, Serialize, Deserialize)]
pub struct OutgoingHttpRequest {
    pub method: String,          // must parse to http::Method
    pub version: Option<String>, // must parse to http::Version
    pub url: String,             // must parse to url::Url
    pub headers: HashMap<String, String>,
    // BODY is stored in the payload, as bytes
    // TIMEOUT is stored in the message expect_response
}

/// HTTP Response type that can be shared over WASM boundary to apps.
#[derive(Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    // BODY is stored in the payload, as bytes
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseBody {
    pub ipc: Vec<u8>,
    pub payload: Option<Payload>,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpClientError {
    #[error("http_client: request could not be parsed to HttpRequest: {}.", req)]
    BadRequest { req: String },
    #[error("http_client: http method not supported: {}", method)]
    BadMethod { method: String },
    #[error("http_client: url could not be parsed: {}", url)]
    BadUrl { url: String },
    #[error("http_client: http version not supported: {}", version)]
    BadVersion { version: String },
    #[error("http_client: failed to execute request {}", error)]
    RequestFailed { error: String },
}

/// Request type sent to `http_server:sys:uqbar` in order to configure it.
/// You can also send [`WebSocketPush`], which allows you to push messages
/// across an existing open WebSocket connection.
///
/// If a response is expected, all HttpServerActions will return a Response
/// with the shape Result<(), HttpServerActionError> serialized to JSON.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpServerAction {
    /// Bind expects a payload if and only if `cache` is TRUE. The payload should
    /// be the static file to serve at this path.
    Bind {
        path: String,
        authenticated: bool,
        local_only: bool,
        cache: bool,
    },
    /// Expects a payload containing the WebSocket message bytes to send.
    WebSocketPush {
        channel_id: u64,
        message_type: WsMessageType,
    },
    /// Processes can both SEND and RECEIVE this kind of request. Sending will
    /// close a socket the process controls. Receiving will indicate that the
    /// client closed the socket.
    WebSocketClose(u64),
}

/// The possible message types for WebSocketPush. Ping and Pong are limited to 125 bytes
/// by the WebSockets protocol. Text will be sent as a Text frame, with the payload bytes
/// being the UTF-8 encoding of the string. Binary will be sent as a Binary frame containing
/// the unmodified payload bytes.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum WsMessageType {
    Text,
    Binary,
    Ping,
    Pong,
}

/// Part of the Response type issued by http_server
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpServerError {
    #[error(
        "http_server: request could not be parsed to HttpServerAction: {}.",
        req
    )]
    BadRequest { req: String },
    #[error("http_server: action expected payload")]
    NoPayload,
    #[error("http_server: path binding error: {:?}", error)]
    PathBindError { error: String },
    #[error("http_server: WebSocket error: {:?}", error)]
    WebSocketPushError { error: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WebSocketClientMessage {
    /// Must be the first message sent along a newly-opened WebSocket connection.
    WsRegister(WsRegister),
    WsMessage(WsMessage),
    EncryptedWsMessage(EncryptedWsMessage),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegister {
    pub ws_auth_token: String,
    pub auth_token: String,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub username: String,
    pub expiration: u64,
}
