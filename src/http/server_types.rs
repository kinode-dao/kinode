use crate::types::LazyLoadBlob;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// HTTP Request type that can be shared over WASM boundary to apps.
/// This is the one you receive from the `http_server:sys:nectar` service.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpServerRequest {
    Http(IncomingHttpRequest),
    /// Processes will receive this kind of request when a client connects to them.
    /// If a process does not want this websocket open, they should issue a *request*
    /// containing a [`type@HttpServerAction::WebSocketClose`] message and this channel ID.
    WebSocketOpen {
        path: String,
        channel_id: u32,
    },
    /// Processes can both SEND and RECEIVE this kind of request
    /// (send as [`type@HttpServerAction::WebSocketPush`]).
    /// When received, will contain the message bytes as lazy_load_blob.
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    /// Receiving will indicate that the client closed the socket. Can be sent to close
    /// from the server-side, as [`type@HttpServerAction::WebSocketClose`].
    WebSocketClose(u32),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IncomingHttpRequest {
    pub source_socket_addr: Option<String>, // will parse to SocketAddr
    pub method: String,                     // will parse to http::Method
    pub url: String,                        // will parse to url::Url
    pub headers: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    // BODY is stored in the lazy_load_blob, as bytes
}

/// HTTP Response type that can be shared over WASM boundary to apps.
/// Respond to [`IncomingHttpRequest`] with this type.
#[derive(Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    // BODY is stored in the lazy_load_blob, as bytes
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseBody {
    pub body: Vec<u8>,
    pub lazy_load_blob: Option<LazyLoadBlob>,
}

/// Request type sent to `http_server:sys:nectar` in order to configure it.
/// You can also send [`type@HttpServerAction::WebSocketPush`], which
/// allows you to push messages across an existing open WebSocket connection.
///
/// If a response is expected, all HttpServerActions will return a Response
/// with the shape Result<(), HttpServerActionError> serialized to JSON.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpServerAction {
    /// Bind expects a lazy_load_blob if and only if `cache` is TRUE. The lazy_load_blob should
    /// be the static file to serve at this path.
    Bind {
        path: String,
        /// Set whether the HTTP request needs a valid login cookie, AKA, whether
        /// the user needs to be logged in to access this path.
        authenticated: bool,
        /// Set whether requests can be fielded from anywhere, or only the loopback address.
        local_only: bool,
        /// Set whether to bind the lazy_load_blob statically to this path. That is, take the
        /// lazy_load_blob bytes and serve them as the response to any request to this path.
        cache: bool,
    },
    /// SecureBind expects a lazy_load_blob if and only if `cache` is TRUE. The lazy_load_blob should
    /// be the static file to serve at this path.
    ///
    /// SecureBind is the same as Bind, except that it forces requests to be made from
    /// the unique subdomain of the process that bound the path. These requests are
    /// *always* authenticated, and *never* local_only. The purpose of SecureBind is to
    /// serve elements of an app frontend or API in an exclusive manner, such that other
    /// apps installed on this node cannot access them. Since the subdomain is unique, it
    /// will require the user to be logged in separately to the general domain authentication.
    SecureBind {
        path: String,
        /// Set whether to bind the lazy_load_blob statically to this path. That is, take the
        /// lazy_load_blob bytes and serve them as the response to any request to this path.
        cache: bool,
    },
    /// Bind a path to receive incoming WebSocket connections.
    /// Doesn't need a cache since does not serve assets.
    WebSocketBind {
        path: String,
        authenticated: bool,
        encrypted: bool,
    },
    /// SecureBind is the same as Bind, except that it forces new connections to be made
    /// from the unique subdomain of the process that bound the path. These are *always*
    /// authenticated. Since the subdomain is unique, it will require the user to be
    /// logged in separately to the general domain authentication.
    WebSocketSecureBind { path: String, encrypted: bool },
    /// Processes will RECEIVE this kind of request when a client connects to them.
    /// If a process does not want this websocket open, they should issue a *request*
    /// containing a [`type@HttpServerAction::WebSocketClose`] message and this channel ID.
    WebSocketOpen { path: String, channel_id: u32 },
    /// When sent, expects a lazy_load_blob containing the WebSocket message bytes to send.
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    /// Sending will close a socket the process controls.
    WebSocketClose(u32),
}

/// The possible message types for WebSocketPush. Ping and Pong are limited to 125 bytes
/// by the WebSockets protocol. Text will be sent as a Text frame, with the lazy_load_blob bytes
/// being the UTF-8 encoding of the string. Binary will be sent as a Binary frame containing
/// the unmodified lazy_load_blob bytes.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum WsMessageType {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

/// Part of the Response type issued by http_server
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpServerError {
    #[error(
        "http_server: request could not be parsed to HttpServerAction: {}.",
        req
    )]
    BadRequest { req: String },
    #[error("http_server: action expected blob")]
    NoBlob,
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
    pub channel_id: u32,
    // TODO symmetric key exchange here
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub username: String,
    pub expiration: u64,
}
