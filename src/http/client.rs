use crate::http::server_types::*;
use crate::http::client_types::*;
use crate::types::*;
use anyhow::Result;
use dashmap::DashMap;
use ethers_providers::StreamExt;
use futures::stream::{SplitSink, SplitStream};
use futures::SinkExt;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage};
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

// Test http_client with these commands in the terminal
// !message our http_client {"method": "GET", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {}}
// !message our http_client {"method": "POST", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}
// !message our http_client {"method": "PUT", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}

// Outgoing WebSocket connections are stored by the source process ID and the channel_id
type WebSocketId = (ProcessId, u32);
type WebSocketMap = DashMap<
    WebSocketId,
    SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, tungstenite::Message>,
>;
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
        message:
            Message::Request(Request {
                expects_response,
                body,
                ..
            }),
        lazy_load_blob: blob,
        ..
    }) = recv_in_client.recv().await
    {
        let Ok(request) = serde_json::from_slice::<HttpClientAction>(&body) else {
            // Send a "BadRequest" error
            http_error_message(
                our_name.clone(),
                id,
                rsvp.unwrap_or(source),
                expects_response,
                HttpClientError::BadRequest {
                    req: String::from_utf8(body).unwrap_or_default(),
                },
                send_to_loop.clone(),
            )
            .await;
            continue;
        };

        let our = our_name.clone();
        let target = rsvp.unwrap_or(source);

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
                (false, Ok(HttpClientResponse::Http(HttpResponse {
                    status: 200,
                    headers: HashMap::new(),
                })))
            }
            HttpClientAction::WebSocketOpen {
                url,
                headers,
                channel_id,
            } => {
                (true, connect_websocket(
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
                .await)
            }
            HttpClientAction::WebSocketPush {
                channel_id,
                message_type,
            } => {
                (true, send_ws_push(target.clone(), channel_id, message_type, blob, ws_streams.clone()).await)
            },
            HttpClientAction::WebSocketClose { channel_id } => {
                (true, close_ws_connection(target.clone(), channel_id, ws_streams.clone(), print_tx.clone()).await)
            }
        };

        if is_ws {
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our_name.to_string(),
                        process: ProcessId::new(Some("http_client"), "sys", "nectar"),
                    },
                    target: target.clone(),
                    rsvp: None,
                    message: Message::Response((
                        Response {
                            inherit: false,
                            body: serde_json::to_vec::<Result<HttpClientResponse, HttpClientError>>(
                                &result
                            )
                            .unwrap(),
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
    Err(anyhow::anyhow!("http_client: loop died"))
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

    let Ok(url) = url::Url::parse(url) else {
        return Err(HttpClientError::BadUrl {
            url: url.to_string(),
        });
    };

    let Ok(mut req) = url.clone().into_client_request() else {
        return Err(HttpClientError::BadRequest {
            req: "failed to parse url into client request".into(),
        });
    };

    let req_headers = req.headers_mut();
    for (key, value) in headers.clone() {
        req_headers.insert(
            HeaderName::from_bytes(key.as_bytes()).unwrap(),
            HeaderValue::from_str(&value).unwrap(),
        );
    }

    let ws_stream = match connect_async(req).await {
        Ok((ws_stream, _)) => ws_stream,
        Err(_) => {
            return Err(HttpClientError::WsOpenFailed {
                url: url.to_string(),
            });
        }
    };

    let (sink, stream) = ws_stream.split();

    if let Some(mut sink) = ws_streams.get_mut(&(target.process.clone(), channel_id)) {
        let _ = sink.close().await;
    }

    ws_streams.insert((target.process.clone(), channel_id), sink);

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
                // Handle different types of messages here
                let (body, blob) = match msg {
                    TungsteniteMessage::Text(text) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Text,
                        },
                        Some(LazyLoadBlob {
                            mime: Some("text/plain".into()),
                            bytes: text.into_bytes(),
                        }),
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
                    ),
                    TungsteniteMessage::Close(_) => {
                        // remove the websocket from the map
                        ws_streams.remove(&(target.process.clone(), channel_id));

                        (HttpClientRequest::WebSocketClose { channel_id }, None)
                    }
                    TungsteniteMessage::Ping(_) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Ping,
                        },
                        None,
                    ),
                    TungsteniteMessage::Pong(_) => (
                        HttpClientRequest::WebSocketPush {
                            channel_id,
                            message_type: WsMessageType::Pong,
                        },
                        None,
                    ),
                    _ => {
                        // should never get a TungsteniteMessage::Frame, ignore if we do
                        continue;
                    }
                };

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
            Err(e) => {
                println!("WebSocket Client Error ({}): {:?}", channel_id, e);

                // The connection was closed/reset by the remote server, so we'll remove and close it
                if let Some(mut ws_sink) = ws_streams.get_mut(&(target.process.clone(), channel_id))
                {
                    // Close the stream. The stream is closed even on error.
                    let _ = ws_sink.close().await;
                }
                ws_streams.remove(&(target.process.clone(), channel_id));

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

    let _ = print_tx
        .send(Printout {
            verbosity: 1,
            content: format!("http_client: building {req_method} request to {}", req.url),
        })
        .await;

    let mut request_builder = client.request(req_method, req.url);

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

    if let Some(blob) = body {
        request_builder = request_builder.body(blob.bytes);
    }

    let Ok(request) = request_builder
        .headers(deserialize_headers(req.headers))
        .build()
    else {
        http_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::RequestFailed {
                error: "failed to build request".into(),
            },
            send_to_loop,
        )
        .await;
        return;
    };

    match client.execute(request).await {
        Ok(response) => {
            let _ = print_tx
                .send(Printout {
                    verbosity: 1,
                    content: "http_client: executed request, got response".to_string(),
                })
                .await;
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our.to_string(),
                        process: ProcessId::new(Some("http_client"), "sys", "nectar"),
                    },
                    target,
                    rsvp: None,
                    message: Message::Response((
                        Response {
                            inherit: false,
                            body:
                                serde_json::to_vec::<Result<HttpClientResponse, HttpClientError>>(
                                    &Ok(HttpClientResponse::Http(HttpResponse {
                                        status: response.status().as_u16(),
                                        headers: serialize_headers(response.headers()),
                                    })),
                                )
                                .unwrap(),
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
                .send(Printout {
                    verbosity: 1,
                    content: "http_client: executed request but got error".to_string(),
                })
                .await;
            http_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::RequestFailed {
                    error: e.to_string(),
                },
                send_to_loop,
            )
            .await;
        }
    }
}

//
//  helpers
//

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

fn serialize_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();
    for (key, value) in headers.iter() {
        let key_str = to_pascal_case(key.as_ref());
        let value_str = value.to_str().unwrap_or("").to_string();
        hashmap.insert(key_str, value_str);
    }
    hashmap
}

fn deserialize_headers(hashmap: HashMap<String, String>) -> HeaderMap {
    let mut header_map = HeaderMap::new();
    for (key, value) in hashmap {
        let key_bytes = key.as_bytes();
        let key_name = HeaderName::from_bytes(key_bytes).unwrap();
        let value_header = HeaderValue::from_str(&value).unwrap();
        header_map.insert(key_name, value_header);
    }
    header_map
}

async fn http_error_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    error: HttpClientError,
    send_to_loop: MessageSender,
) {
    if expects_response.is_some() {
        let _ = send_to_loop
            .send(KernelMessage {
                id,
                source: Address {
                    node: our.to_string(),
                    process: ProcessId::new(Some("http_client"), "sys", "nectar"),
                },
                target,
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        body: serde_json::to_vec::<Result<HttpResponse, HttpClientError>>(&Err(
                            error,
                        ))
                        .unwrap(),
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

async fn send_ws_push(
    target: Address,
    channel_id: u32,
    message_type: WsMessageType,
    blob: Option<LazyLoadBlob>,
    ws_streams: WebSocketStreams,
) -> Result<HttpClientResponse, HttpClientError> {
    let Some(mut ws_stream) = ws_streams.get_mut(&(target.process.clone(), channel_id)) else {
        return Err(HttpClientError::WsPushFailed {
            req: format!("channel_id {} not found", channel_id),
        });
    };

    let _ = match message_type {
        WsMessageType::Text => {
            let Some(blob) = blob else {
                return Err(HttpClientError::WsPushFailed {
                    req: "no blob".into(),
                });
            };

            let Ok(text) = String::from_utf8(blob.bytes) else {
                return Err(HttpClientError::WsPushFailed {
                    req: "failed to convert blob to string".into(),
                });
            };

            ws_stream.send(TungsteniteMessage::Text(text)).await
        }
        WsMessageType::Binary => {
            let Some(blob) = blob else {
                return Err(HttpClientError::WsPushFailed {
                    req: "no blob".into(),
                });
            };

            ws_stream.send(TungsteniteMessage::Binary(blob.bytes)).await
        }
        WsMessageType::Ping => ws_stream.send(TungsteniteMessage::Ping(vec![])).await,
        WsMessageType::Pong => ws_stream.send(TungsteniteMessage::Pong(vec![])).await,
        WsMessageType::Close => ws_stream.send(TungsteniteMessage::Close(None)).await,
    };

    Ok(HttpClientResponse::WebSocketAck)
}

async fn close_ws_connection(
    target: Address,
    channel_id: u32,
    ws_streams: WebSocketStreams,
    _print_tx: PrintSender,
) -> Result<HttpClientResponse, HttpClientError> {
    let Some(mut ws_sink) = ws_streams.get_mut(&(target.process.clone(), channel_id)) else {
        return Err(HttpClientError::WsCloseFailed { channel_id });
    };

    // Close the stream. The stream is closed even on error.
    let _ = ws_sink.close().await;

    Ok(HttpClientResponse::WebSocketAck)
}

async fn handle_ws_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    body: HttpClientRequest,
    blob: Option<LazyLoadBlob>,
    send_to_loop: MessageSender,
) {
    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: our.to_string(),
                process: ProcessId::new(Some("http_client"), "sys", "nectar"),
            },
            target,
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                body: serde_json::to_vec::<HttpClientRequest>(&body).unwrap(),
                expects_response: None,
                metadata: None,
                capabilities: vec![],
            }),
            lazy_load_blob: blob,
        })
        .await;
}
