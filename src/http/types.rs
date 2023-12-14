use crate::types::Payload;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// HTTP Request type that can be shared over WASM boundary to apps.
/// This is the one you receive from the `http_server:sys:uqbar` service.
#[derive(Debug, Serialize, Deserialize)]
pub struct IncomingHttpRequest {
    pub source_socket_addr: Option<String>, // will parse to SocketAddr
    pub method: String,                     // will parse to http::Method
    pub raw_path: String,
    pub headers: HashMap<String, String>,
    // BODY is stored in the payload, as bytes
}

/// HTTP Request type that can be shared over WASM boundary to apps.
/// This is the one you send to the `http_client:sys:uqbar` service.
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
/// Respond to [`IncomingHttpRequest`] with this type.
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
    /// Processes will RECEIVE this kind of request when a client connects to them.
    /// If a process does not want this websocket open, they can respond with an
    /// [`enum@HttpServerAction::WebSocketClose`] message.
    WebSocketOpen(u64),
    /// Processes can both SEND and RECEIVE this kind of request.
    /// When sent, expects a payload containing the WebSocket message bytes to send.
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

/// Structure sent from client websocket to this server upon opening a new connection.
/// After this is sent, depending on the `encrypted` flag, the channel will either be
/// open to send and receive plaintext messages or messages encrypted with a symmetric
/// key derived from the JWT.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegister {
    pub auth_token: String,
    pub target_process: String,
    pub encrypted: bool, // TODO symmetric key exchange here if true
}

/// Structure sent from this server to client websocket upon opening a new connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegisterResponse {
    pub channel_id: u64,
    // TODO symmetric key exchange here
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub username: String,
    pub expiration: u64,
}
