use crate::core::LazyLoadBlob;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// HTTP Request received from the `http-server:distro:sys` service as a
/// result of either an HTTP or WebSocket binding, created via [`HttpServerAction`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HttpServerRequest {
    Http(IncomingHttpRequest),
    /// Processes will receive this kind of request when a client connects to them.
    /// If a process does not want this websocket open, they should issue a *request*
    /// containing a [`HttpServerAction::WebSocketClose`] message and this channel ID.
    WebSocketOpen {
        path: String,
        channel_id: u32,
    },
    /// Processes can both SEND and RECEIVE this kind of request
    /// (send as [`HttpServerAction::WebSocketPush`]).
    /// When received, will contain the message bytes as lazy_load_blob.
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    /// Receiving will indicate that the client closed the socket. Can be sent to close
    /// from the server-side, as [`type@HttpServerAction::WebSocketClose`].
    WebSocketClose(u32),
}

/// An HTTP request routed to a process as a result of a binding.
///
/// BODY is stored in the lazy_load_blob, as bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IncomingHttpRequest {
    /// will parse to SocketAddr
    pub source_socket_addr: Option<String>,
    /// will parse to http::Method
    pub method: String,
    /// will parse to url::Url
    pub url: String,
    /// the matching path that was bound
    pub bound_path: String,
    /// will parse to http::HeaderMap
    pub headers: HashMap<String, String>,
    pub url_params: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
}

/// HTTP Response type that can be shared over Wasm boundary to apps.
/// Respond to [`IncomingHttpRequest`] with this type.
///
/// BODY is stored in the lazy_load_blob, as bytes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponseBody {
    pub body: Vec<u8>,
    pub lazy_load_blob: Option<LazyLoadBlob>,
}

/// Request type sent to `http-server:distro:sys` in order to configure it.
///
/// If a response is expected, all actions will return a Response
/// with the shape `Result<(), HttpServerActionError>` serialized to JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Unbind a previously-bound HTTP path
    Unbind { path: String },
    /// Bind a path to receive incoming WebSocket connections.
    /// Doesn't need a cache since does not serve assets.
    WebSocketBind {
        path: String,
        authenticated: bool,
        extension: bool,
    },
    /// SecureBind is the same as Bind, except that it forces new connections to be made
    /// from the unique subdomain of the process that bound the path. These are *always*
    /// authenticated. Since the subdomain is unique, it will require the user to be
    /// logged in separately to the general domain authentication.
    WebSocketSecureBind { path: String, extension: bool },
    /// Unbind a previously-bound WebSocket path
    WebSocketUnbind { path: String },
    /// Processes will RECEIVE this kind of request when a client connects to them.
    /// If a process does not want this websocket open, they should issue a *request*
    /// containing a [`HttpServerAction::WebSocketClose`] message and this channel ID.
    WebSocketOpen { path: String, channel_id: u32 },
    /// When sent, expects a lazy_load_blob containing the WebSocket message bytes to send.
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    /// When sent, expects a `lazy_load_blob` containing the WebSocket message bytes to send.
    /// Modifies the `lazy_load_blob` by placing into `WebSocketExtPushData` with id taken from
    /// this `KernelMessage` and `hyperware_message_type` set to `desired_reply_type`.
    WebSocketExtPushOutgoing {
        channel_id: u32,
        message_type: WsMessageType,
        desired_reply_type: MessageType,
    },
    /// For communicating with the ext.
    /// Hyperdrive's http-server sends this to the ext after receiving `WebSocketExtPushOutgoing`.
    /// Upon receiving reply with this type from ext, http-server parses, setting:
    /// * id as given,
    /// * message type as given (Request or Response),
    /// * body as HttpServerRequest::WebSocketPush,
    /// * blob as given.
    WebSocketExtPushData {
        id: u64,
        hyperware_message_type: MessageType,
        blob: Vec<u8>,
    },
    /// Sending will close a socket the process controls.
    WebSocketClose(u32),
}

/// Whether the WebSocketPush is a request or a response.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum MessageType {
    Request,
    Response,
}

/// The possible message types for [`HttpServerRequest::WebSocketPush`].
/// Ping and Pong are limited to 125 bytes by the WebSockets protocol.
/// Text will be sent as a Text frame, with the lazy_load_blob bytes
/// being the UTF-8 encoding of the string. Binary will be sent as a
/// Binary frame containing the unmodified lazy_load_blob bytes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum WsMessageType {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

/// Part of the Response type issued by `http-server:distro:sys`
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpServerError {
    #[error("request could not be deserialized to valid HttpServerRequest")]
    MalformedRequest,
    #[error("action expected blob")]
    NoBlob,
    #[error("path binding error: invalid source process")]
    InvalidSourceProcess,
    #[error("WebSocket error: ping/pong message too long")]
    WsPingPongTooLong,
    #[error("WebSocket error: channel not found")]
    WsChannelNotFound,
}

/// Structure sent from client websocket to this server upon opening a new connection.
/// After this is sent the channel will be open to send and receive plaintext messages.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegister {
    pub auth_token: String,
    pub target_process: String,
}

/// Structure sent from this server to client websocket upon opening a new connection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegisterResponse {
    pub channel_id: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub username: String,
    pub subdomain: Option<String>,
    pub expiration: u64,
}
