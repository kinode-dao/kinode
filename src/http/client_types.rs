use crate::http::server_types::{HttpResponse, WsMessageType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

// TODO: add documentation similar to server_types

/// Request type that can be shared over WASM boundary to apps.
/// This is the one you send to the `http_client:distro:sys` service.
#[derive(Debug, Serialize, Deserialize)]
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

/// HTTP Request type that can be shared over WASM boundary to apps.
/// This is the one you send to the `http_client:distro:sys` service.
#[derive(Debug, Serialize, Deserialize)]
pub struct OutgoingHttpRequest {
    pub method: String,          // must parse to http::Method
    pub version: Option<String>, // must parse to http::Version
    pub url: String,             // must parse to url::Url
    pub headers: HashMap<String, String>,
    // BODY is stored in the lazy_load_blob, as bytes
    // TIMEOUT is stored in the message expect_response
}

/// WebSocket Client Request type that can be shared over WASM boundary to apps.
/// This comes from an open websocket client connection in the `http_client:distro:sys` service.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpClientRequest {
    WebSocketPush {
        channel_id: u32,
        message_type: WsMessageType,
    },
    WebSocketClose {
        channel_id: u32,
    },
}

/// HTTP Client Response type that can be shared over WASM boundary to apps.
/// This is the one you receive from the `http_client:distro:sys` service.
#[derive(Debug, Serialize, Deserialize)]
pub enum HttpClientResponse {
    Http(HttpResponse),
    WebSocketAck,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpClientError {
    // HTTP errors, may also be applicable to OutgoingWebSocketClientRequest::Open
    #[error("http_client: request is not valid HttpClientRequest: {}.", req)]
    BadRequest { req: String },
    #[error("http_client: http method not supported: {}", method)]
    BadMethod { method: String },
    #[error("http_client: url could not be parsed: {}", url)]
    BadUrl { url: String },
    #[error("http_client: http version not supported: {}", version)]
    BadVersion { version: String },
    #[error("http_client: failed to execute request {}", error)]
    RequestFailed { error: String },

    // WebSocket errors
    #[error("websocket_client: failed to open connection {}", url)]
    WsOpenFailed { url: String },
    #[error("websocket_client: failed to send message {}", req)]
    WsPushFailed { req: String },
    #[error("websocket_client: failed to close connection {}", channel_id)]
    WsCloseFailed { channel_id: u32 },
}
