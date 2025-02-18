use anyhow::Result;
use dashmap::DashMap;
use futures::stream::{SplitSink, SplitStream};
use futures::SinkExt;
use futures::StreamExt;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage};
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use lib::types::{core::*, http_client::*, http_server::*};

// Test http-client with these commands in the terminal
// m our@http-client:distro:sys '{"method": "GET", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {}}'
// m our@http-client:distro:sys '{"method": "POST", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}'
// m our@http-client:distro:sys '{"method": "PUT", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}'

/// WebSocket client connections are mapped by a tuple of ProcessId and
/// a process-supplied channel_id (u32)
type WebSocketId = (ProcessId, u32);
type WebSocketMap = DashMap<
    WebSocketId,
    SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, tungstenite::Message>,
>;
/// The WebSocket streams are split into sink and stream
/// so that both incoming and outgoing pushes can be routed appropriately
type WebSocketStreams = Arc<WebSocketMap>;

pub async fn http_client(
    our_name: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    let client = reqwest::Client::new();
    let our_name = Arc::new(our_name);

    let ws_streams: WebSocketStreams = Arc::new(DashMap::new());

    while let Some(KernelMessage {
        id,
        source,
        rsvp,
        message,
        lazy_load_blob: blob,
        ..
    }) = recv_in_client.recv().await
    {
        let Message::Request(Request {
            body,
            expects_response,
            ..
        }) = message
        else {
            continue;
        };
        // Check that the incoming request body is a HttpClientAction
        let Ok(request) = serde_json::from_slice::<HttpClientAction>(&body) else {
            // Send a "BadRequest" error if deserialization fails
            http_error_message(
                our_name.clone(),
                id,
                rsvp.unwrap_or(source),
                expects_response,
                HttpClientError::MalformedRequest,
                send_to_loop.clone(),
            )
            .await;
            continue;
        };

        let our = our_name.clone();
        // target is the source or specified rsvp Address to which
        // responses or incoming WS messages will be routed
        let target = rsvp.unwrap_or(source);

        // Handle the request, returning if the request was a WS request
        let (is_ws, result) = match request {
            HttpClientAction::Http(req) => {
                tokio::spawn(handle_http_request(
                    our,
                    id,
                    target.clone(),
                    expects_response,
                    req,
                    blob,
                    client.clone(),
                    send_to_loop.clone(),
                    print_tx.clone(),
                ));
                (
                    false,
                    Ok(HttpClientResponse::Http(HttpResponse {
                        status: 200,
                        headers: HashMap::new(),
                    })),
                )
            }
            HttpClientAction::WebSocketOpen {
                url,
                headers,
                channel_id,
            } => (
                true,
                connect_websocket(
                    our,
                    id,
                    target.clone(),
                    &url,
                    headers,
                    channel_id,
                    ws_streams.clone(),
                    send_to_loop.clone(),
                    print_tx.clone(),
                )
                .await,
            ),
            HttpClientAction::WebSocketPush {
                channel_id,
                message_type,
            } => (
                true,
                send_ws_push(
                    target.clone(),
                    channel_id,
                    message_type,
                    blob,
                    ws_streams.clone(),
                )
                .await,
            ),
            HttpClientAction::WebSocketClose { channel_id } => (
                true,
                close_ws_connection(
                    target.clone(),
                    channel_id,
                    ws_streams.clone(),
                    print_tx.clone(),
                )
                .await,
            ),
        };

        // If the incoming request was a WS request, send a response
        // HTTP responses are handled in the handle_http_request function
        if is_ws {
            let Ok(body) =
                serde_json::to_vec::<Result<HttpClientResponse, HttpClientError>>(&result)
            else {
                continue;
            };
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address::new(our_name.as_str(), ("http-client", "distro", "sys")),
                    target: target.clone(),
                    rsvp: None,
                    message: Message::Response((
                        Response {
                            inherit: false,
                            body,
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )),
                    lazy_load_blob: None,
                })
                .await;
        }
    }
    Err(anyhow::anyhow!("http-client: loop died"))
}

async fn connect_websocket(
    our: Arc<String>,
    id: u64,
    target: Address,
    url: &str,
    headers: HashMap<String, String>,
    channel_id: u32,
    ws_streams: WebSocketStreams,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) -> Result<HttpClientResponse, HttpClientError> {
    let print_tx_clone = print_tx.clone();

    // First check the URL
    let Ok(url) = url::Url::parse(url) else {
        return Err(HttpClientError::BadUrl {
            url: url.to_string(),
        });
    };

    let Ok(mut req) = url.clone().into_client_request() else {
        return Err(HttpClientError::WsOpenFailed {
            url: url.to_string(),
        });
    };

    // Add headers to the request
    let req_headers = req.headers_mut();
    for (key, value) in headers.clone() {
        if let Ok(key_name) = HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(value_header) = HeaderValue::from_str(&value) {
                req_headers.insert(key_name, value_header);
            }
        }
    }

    // Connect the WebSocket
    let ws_stream = match connect_async(req).await {
        Ok((ws_stream, _)) => ws_stream,
        Err(e) => {
            let _ = print_tx
                .send(Printout::new(
                    1,
                    HTTP_CLIENT_PROCESS_ID.clone(),
                    format!("http-client: underlying lib connection error {e:?}"),
                ))
                .await;

            return Err(HttpClientError::WsOpenFailed {
                url: url.to_string(),
            });
        }
    };

    // Split the WebSocket connection
    let (sink, stream) = ws_stream.split();

    // Close any existing sink with the same ProcessId and channel_id
    if let Some(mut sink) = ws_streams.get_mut(&(target.process.clone(), channel_id)) {
        let _ = sink.close().await;
    }

    // Insert the sink (send or push part of the WebSocket connection)
    ws_streams.insert((target.process.clone(), channel_id), sink);

    // Spawn a new tokio process to listen to incoming WS events on the stream
    tokio::spawn(listen_to_stream(
        our.clone(),
        id,
        target.clone(),
        channel_id,
        stream,
        ws_streams,
        send_to_loop.clone(),
        print_tx_clone,
    ));

    Ok(HttpClientResponse::WebSocketAck)
}

async fn listen_to_stream(
    our: Arc<String>,
    id: u64,
    target: Address,
    channel_id: u32,
    mut stream: SplitStream<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>>,
    ws_streams: WebSocketStreams,
    send_to_loop: MessageSender,
    _print_tx: PrintSender,
) {
    while let Some(message) = stream.next().await {
        match message {
            Ok(msg) => {
                // Handle different types of incoming WebSocket messages
                let (body, blob, should_exit) = match msg {
                    TungsteniteMessage::Text(text) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Text,
                        },
                        Some(LazyLoadBlob {
                            mime: Some("text/plain".into()),
                            bytes: text.into_bytes(),
                        }),
                        false,
                    ),
                    TungsteniteMessage::Binary(bytes) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Binary,
                        },
                        Some(LazyLoadBlob {
                            mime: Some("application/octet-stream".into()),
                            bytes,
                        }),
                        false,
                    ),
                    TungsteniteMessage::Close(_) => {
                        // remove the websocket from the map
                        ws_streams.remove(&(target.process.clone(), channel_id));

                        (HttpClientRequest::WebSocketClose { channel_id }, None, true)
                    }
                    TungsteniteMessage::Ping(_) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Ping,
                        },
                        None,
                        false,
                    ),
                    TungsteniteMessage::Pong(_) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Pong,
                        },
                        None,
                        false,
                    ),
                    _ => {
                        // should never get a TungsteniteMessage::Frame, ignore if we do
                        continue;
                    }
                };

                if ws_streams.contains_key(&(target.process.clone(), channel_id)) || should_exit {
                    handle_ws_message(
                        our.clone(),
                        id,
                        target.clone(),
                        body,
                        blob,
                        send_to_loop.clone(),
                    )
                    .await;
                }

                if should_exit {
                    break;
                }
            }
            Err(e) => {
                println!("WebSocket Client Error ({}): {:?}", channel_id, e);

                // The connection was closed/reset by the remote server, so we'll remove and close it
                if let Some(mut ws_sink) = ws_streams.get_mut(&(target.process.clone(), channel_id))
                {
                    // Close the stream. The stream is closed even on error.
                    let _ = ws_sink.close().await;
                }
                // Remove the stream from the map
                ws_streams.remove(&(target.process.clone(), channel_id));

                // Notify the originating process that the connection was closed
                handle_ws_message(
                    our.clone(),
                    id,
                    target.clone(),
                    HttpClientRequest::WebSocketClose { channel_id },
                    None,
                    send_to_loop.clone(),
                )
                .await;

                break;
            }
        }
    }
}

async fn handle_http_request(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    req: OutgoingHttpRequest,
    body: Option<LazyLoadBlob>,
    client: reqwest::Client,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    // Parse the HTTP Method
    let Ok(req_method) = http::Method::from_bytes(req.method.as_bytes()) else {
        http_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::BadMethod { method: req.method },
            send_to_loop,
        )
        .await;
        return;
    };

    let Ok(url) = url::Url::parse(&req.url) else {
        http_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::BadUrl { url: req.url },
            send_to_loop,
        )
        .await;
        return;
    };

    let _ = print_tx
        .send(Printout::new(
            2,
            HTTP_CLIENT_PROCESS_ID.clone(),
            format!("http-client: {req_method} request to {}", url),
        ))
        .await;

    // Build the request
    let mut request_builder = client.request(req_method, url);

    if let Some(version) = req.version {
        request_builder = match version.as_str() {
            "HTTP/0.9" => request_builder.version(http::Version::HTTP_09),
            "HTTP/1.0" => request_builder.version(http::Version::HTTP_10),
            "HTTP/1.1" => request_builder.version(http::Version::HTTP_11),
            "HTTP/2.0" => request_builder.version(http::Version::HTTP_2),
            "HTTP/3.0" => request_builder.version(http::Version::HTTP_3),
            _ => {
                http_error_message(
                    our,
                    id,
                    target,
                    expects_response,
                    HttpClientError::BadVersion { version },
                    send_to_loop,
                )
                .await;
                return;
            }
        }
    }

    // Add the body as appropriate
    if let Some(blob) = body {
        request_builder = request_builder.body(blob.bytes);
    }

    // Add the headers
    let build = request_builder
        .headers(deserialize_headers(req.headers))
        .build();
    if let Err(e) = build {
        http_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::BuildRequestFailed(e.to_string()),
            send_to_loop,
        )
        .await;
        return;
    };

    // Send the HTTP request
    match client.execute(build.unwrap()).await {
        Ok(response) => {
            // Handle the response and forward to the target process
            let Ok(body) = serde_json::to_vec::<Result<HttpClientResponse, HttpClientError>>(&Ok(
                HttpClientResponse::Http(HttpResponse {
                    status: response.status().as_u16(),
                    headers: serialize_headers(response.headers()),
                }),
            )) else {
                return;
            };
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our.to_string(),
                        process: ProcessId::new(Some("http-client"), "distro", "sys"),
                    },
                    target,
                    rsvp: None,
                    message: Message::Response((
                        Response {
                            inherit: false,
                            body,
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )),
                    lazy_load_blob: Some(LazyLoadBlob {
                        mime: None,
                        bytes: response.bytes().await.unwrap_or_default().to_vec(),
                    }),
                })
                .await;
        }
        Err(e) => {
            let _ = print_tx
                .send(Printout::new(
                    2,
                    HTTP_CLIENT_PROCESS_ID.clone(),
                    "http-client: executed request but got error".to_string(),
                ))
                .await;
            // Forward the error to the target process
            http_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::ExecuteRequestFailed(e.to_string()),
                send_to_loop,
            )
            .await;
        }
    }
}

//
//  helpers
//

/// Convert a &str to Pascal-Case (for HTTP headers)
fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("-")
}

// Convert from HeaderMap to HashMap
fn serialize_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();
    for (key, value) in headers.iter() {
        let key_str = to_pascal_case(key.as_ref());
        let value_str = value.to_str().unwrap_or_default().to_string();
        hashmap.insert(key_str, value_str);
    }
    hashmap
}

// Convert from HashMap to HeaderMap
fn deserialize_headers(hashmap: HashMap<String, String>) -> HeaderMap {
    let mut header_map = HeaderMap::new();
    for (key, value) in hashmap {
        let key_bytes = key.as_bytes();
        if let Ok(key_name) = HeaderName::from_bytes(key_bytes) {
            if let Ok(value_header) = HeaderValue::from_str(&value) {
                header_map.insert(key_name, value_header);
            }
        }
    }
    header_map
}

/// Send an HTTP error to a target
async fn http_error_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    error: HttpClientError,
    send_to_loop: MessageSender,
) {
    if expects_response.is_some() {
        let Ok(body) = serde_json::to_vec::<Result<HttpResponse, HttpClientError>>(&Err(error))
        else {
            return;
        };
        let _ = send_to_loop
            .send(KernelMessage {
                id,
                source: Address {
                    node: our.to_string(),
                    process: ProcessId::new(Some("http-client"), "distro", "sys"),
                },
                target,
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        body,
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )),
                lazy_load_blob: None,
            })
            .await;
    }
}

/// Send a WS push to a connection
async fn send_ws_push(
    target: Address,
    channel_id: u32,
    message_type: WsMessageType,
    blob: Option<LazyLoadBlob>,
    ws_streams: WebSocketStreams,
) -> Result<HttpClientResponse, HttpClientError> {
    let Some(mut ws_stream) = ws_streams.get_mut(&(target.process.clone(), channel_id)) else {
        return Err(HttpClientError::WsPushUnknownChannel { channel_id });
    };

    let _ = match message_type {
        WsMessageType::Text => {
            let Some(blob) = blob else {
                return Err(HttpClientError::WsPushNoBlob);
            };

            let Ok(text) = String::from_utf8(blob.bytes) else {
                return Err(HttpClientError::WsPushBadText);
            };

            ws_stream.send(TungsteniteMessage::Text(text)).await
        }
        WsMessageType::Binary => {
            let Some(blob) = blob else {
                return Err(HttpClientError::WsPushNoBlob);
            };

            ws_stream.send(TungsteniteMessage::Binary(blob.bytes)).await
        }
        WsMessageType::Ping => ws_stream.send(TungsteniteMessage::Ping(vec![])).await,
        WsMessageType::Pong => ws_stream.send(TungsteniteMessage::Pong(vec![])).await,
        WsMessageType::Close => ws_stream.send(TungsteniteMessage::Close(None)).await,
    };

    Ok(HttpClientResponse::WebSocketAck)
}

/// Close a WS connection, sending a close event to the sink will also close the stream
async fn close_ws_connection(
    target: Address,
    channel_id: u32,
    ws_streams: WebSocketStreams,
    _print_tx: PrintSender,
) -> Result<HttpClientResponse, HttpClientError> {
    let Some((_, mut ws_sink)) = ws_streams.remove(&(target.process.clone(), channel_id)) else {
        return Err(HttpClientError::WsCloseFailed { channel_id });
    };

    // Close the stream. The stream is closed even on error.
    let _ = ws_sink.close().await;

    Ok(HttpClientResponse::WebSocketAck)
}

/// Forward an incoming WS request from an external source to the corresponding process
async fn handle_ws_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    body: HttpClientRequest,
    blob: Option<LazyLoadBlob>,
    send_to_loop: MessageSender,
) {
    let Ok(body) = serde_json::to_vec::<HttpClientRequest>(&body) else {
        return;
    };
    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: our.to_string(),
                process: ProcessId::new(Some("http-client"), "distro", "sys"),
            },
            target,
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                body,
                expects_response: None,
                metadata: None,
                capabilities: vec![],
            }),
            lazy_load_blob: blob,
        })
        .await;
}
