use crate::http::server_types::{HttpResponse, WsMessageType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// Request type sent to the `http-client:distro:sys` service.
///
/// Always serialized/deserialized as JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HttpClientAction {
    Http(OutgoingHttpRequest),
    WebSocketOpen {
        url: String,
        headers: HashMap<String, String>,
        channel_id: u32,
    },
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    WebSocketClose {
        channel_id: u32,
    },
}

/// HTTP Request type contained in [`HttpClientAction::Http`].
///
/// BODY is stored in the lazy_load_blob, as bytes
///
/// TIMEOUT is stored in the message's `expects_response` value
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutgoingHttpRequest {
    /// must parse to [`http::Method`]
    pub method: String,
    /// must parse to [`http::Version`]
    pub version: Option<String>,
    /// must parse to [`url::Url`]
    pub url: String,
    pub headers: HashMap<String, String>,
}

/// Request that comes from an open WebSocket client connection in the
/// `http-client:distro:sys` service. Be prepared to receive these after
/// using a [`HttpClientAction::WebSocketOpen`] to open a connection.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum HttpClientRequest {
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    WebSocketClose {
        channel_id: u32,
    },
}

/// Response type received from the `http-client:distro:sys` service after
/// sending a successful [`HttpClientAction`] to it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum HttpClientResponse {
    Http(HttpResponse),
    WebSocketAck,
}

#[derive(Clone, Debug, Error, Serialize, Deserialize)]
pub enum HttpClientError {
    // HTTP errors
    #[error("request could not be deserialized to valid HttpClientRequest")]
    MalformedRequest,
    #[error("http method not supported: {method}")]
    BadMethod { method: String },
    #[error("url could not be parsed: {url}")]
    BadUrl { url: String },
    #[error("http version not supported: {version}")]
    BadVersion { version: String },
    #[error("client failed to build request: {0}")]
    BuildRequestFailed(String),
    #[error("client failed to execute request: {0}")]
    ExecuteRequestFailed(String),

    // WebSocket errors
    #[error("could not open connection to {url}")]
    WsOpenFailed { url: String },
    #[error("sent WebSocket push to unknown channel {channel_id}")]
    WsPushUnknownChannel { channel_id: u32 },
    #[error("WebSocket push failed because message had no blob attached")]
    WsPushNoBlob,
    #[error("WebSocket push failed because message type was Text, but blob was not a valid UTF-8 string")]
    WsPushBadText,
    #[error("failed to close connection {channel_id} because it was not open")]
    WsCloseFailed { channel_id: u32 },
}
