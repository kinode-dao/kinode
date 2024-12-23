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

/// HTTP Request type that can be shared over Wasm boundary to apps.
/// This is the one you send to the `http-client:distro:sys` service.
///
/// BODY is stored in the lazy_load_blob, as bytes
///
/// TIMEOUT is stored in the message expect_response value
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
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpClientResponse {
    Http(HttpResponse),
    WebSocketAck,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpClientError {
    // HTTP errors
    #[error("http-client: request is not valid HttpClientRequest: {req}.")]
    BadRequest { req: String },
    #[error("http-client: http method not supported: {method}.")]
    BadMethod { method: String },
    #[error("http-client: url could not be parsed: {url}.")]
    BadUrl { url: String },
    #[error("http-client: http version not supported: {version}.")]
    BadVersion { version: String },
    #[error("http-client: failed to execute request {error}.")]
    RequestFailed { error: String },

    // WebSocket errors
    #[error("http-client: failed to open connection {url}.")]
    WsOpenFailed { url: String },
    #[error("http-client: failed to send message {req}.")]
    WsPushFailed { req: String },
    #[error("http-client: failed to close connection {channel_id}.")]
    WsCloseFailed { channel_id: u32 },
}
