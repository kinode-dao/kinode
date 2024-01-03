use crate::http::types::*;
use crate::types::*;
use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use futures::stream::SplitSink;
use futures::SinkExt;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_tungstenite::tungstenite::{Message as TungsteniteMessage, client::IntoClientRequest};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use ethers_providers::StreamExt;

// Test http_client with these commands in the terminal
// !message our http_client {"method": "GET", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {}}
// !message our http_client {"method": "POST", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}
// !message our http_client {"method": "PUT", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}

// Outgoing WebSocket connections are stored by the source process ID and the channel_id
type WebSocketId = (ProcessId, u32);
type WebSocketMap = HashMap<WebSocketId, SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, tungstenite::Message>>;

pub async fn http_client(
    our_name: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    let client = reqwest::Client::new();
    let our_name = Arc::new(our_name);

    let ws_streams: Arc<Mutex<WebSocketMap>> =
        Arc::new(Mutex::new(HashMap::new()));

    while let Some(KernelMessage {
        id,
        source,
        rsvp,
        message:
            Message::Request(Request {
                expects_response,
                ipc,
                ..
            }),
        payload,
        ..
    }) = recv_in_client.recv().await
    {
        // First check if a WebSocketClientAction, otherwise assume it's an OutgoingHttpRequest
        if let Ok(ws_action) = serde_json::from_slice::<WebSocketClientAction>(&ipc) {
            let ws_streams_clone = Arc::clone(&ws_streams);
            tokio::spawn(handle_websocket_action(
                our_name.clone(),
                id,
                rsvp.unwrap_or(source),
                expects_response,
                ws_action,
                payload,
                ws_streams_clone,
                send_to_loop.clone(),
                print_tx.clone(),
            ));
        } else {
            tokio::spawn(handle_http_request(
                our_name.clone(),
                id,
                rsvp.unwrap_or(source),
                expects_response,
                ipc,
                payload,
                client.clone(),
                send_to_loop.clone(),
                print_tx.clone(),
            ));
        }
    }
    Err(anyhow::anyhow!("http_client: loop died"))
}

async fn handle_websocket_action(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    ws_action: WebSocketClientAction,
    payload: Option<Payload>,
    ws_streams: Arc<Mutex<WebSocketMap>>,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    match ws_action {
        WebSocketClientAction::Open { url, headers, channel_id } => {
            connect_websocket(
                our,
                id,
                target.clone(),
                expects_response,
                &url,
                headers,
                channel_id,
                ws_streams,
                send_to_loop,
                print_tx,
            ).await;
        },
        WebSocketClientAction::Push { channel_id, message_type } => {
            send_ws_push(
                our,
                id,
                target,
                expects_response,
                channel_id,
                message_type,
                payload,
                ws_streams,
                send_to_loop,
            ).await;
        },
        WebSocketClientAction::Close { channel_id } => {
            close_ws_connection(
                our,
                id,
                target,
                expects_response,
                channel_id,
                ws_streams,
                send_to_loop,
            ).await;
        }
    }
}

async fn insert_ws (
    ws_streams: &Arc<Mutex<WebSocketMap>>,
    sink: SplitSink<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>, tungstenite::Message>,
    source: Address,
    channel_id: u32,
) {
    let mut ws_streams = ws_streams.lock().await;

    ws_streams.insert((source.process, channel_id), sink);
}

async fn connect_websocket(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    url: &str,
    headers: HashMap<String, String>,
    channel_id: u32,
    ws_streams: Arc<Mutex<WebSocketMap>>,
    send_to_loop: MessageSender,
    _print_tx: PrintSender,
) {
    let Ok(url) = url::Url::parse(url) else {
        make_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::BadRequest {
                req: "failed to parse url".into(),
            },
            send_to_loop,
        ).await;
        return;
    };

    let Ok(mut req) = url.clone().into_client_request() else {
        make_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::BadRequest {
                req: "failed to parse url into client request".into(),
            },
            send_to_loop,
        ).await;
        return;
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
            make_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::RequestFailed {
                    error: "failed to connect to websocket".into(),
                },
                send_to_loop,
            )
            .await;
            return;
        }
    };

    let (sink, mut stream) = ws_stream.split();

    insert_ws(&ws_streams, sink, target.clone(), channel_id).await;

    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: our.to_string(),
                process: ProcessId::new(Some("http_client"), "sys", "uqbar"),
            },
            target: target.clone(),
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc: serde_json::to_vec::<WebSocketClientAction>(&WebSocketClientAction::Open {
                        url: url.to_string(),
                        headers,
                        channel_id,
                    })
                    .unwrap(),
                    metadata: None,
                },
                None,
            )),
            payload: None,
            signed_capabilities: None,
        })
        .await;

    while let Some(message) = stream.next().await {
        match message {
            Ok(msg) => {
                // Handle different types of messages here
                match msg {
                    TungsteniteMessage::Text(text) => {
                        // send a Request to the target with the text as payload
                        handle_ws_message(
                            our.clone(),
                            id,
                            target.clone(),
                            WebSocketClientAction::Push {
                                channel_id,
                                message_type: WsMessageType::Text,
                            },
                            Some(Payload {
                                mime: Some("text/plain".into()),
                                bytes: text.into_bytes(),
                            }),
                            send_to_loop.clone(),
                        ).await;
                    },
                    TungsteniteMessage::Binary(bytes) => {
                        // send a Request to the target with the binary as payload
                        handle_ws_message(
                            our.clone(),
                            id,
                            target.clone(),
                            WebSocketClientAction::Push {
                                channel_id,
                                message_type: WsMessageType::Binary,
                            },
                            Some(Payload {
                                mime: Some("application/octet-stream".into()),
                                bytes,
                            }),
                            send_to_loop.clone(),
                        ).await;
                    },
                    TungsteniteMessage::Close(_) => {
                        // send a websocket close Request to the target
                        handle_ws_message(
                            our.clone(),
                            id,
                            target.clone(),
                            WebSocketClientAction::Close {
                                channel_id,
                            },
                            None,
                            send_to_loop.clone(),
                        ).await;

                        // remove the websocket from the map
                        let mut ws_streams = ws_streams.lock().await;
                        ws_streams.remove(&(target.process.clone(), channel_id));
                    },
                    _ => (), // Handle other message types as needed
                }
            }
            Err(e) => {
                println!("WebSocket Client Error ({}): {:?}", channel_id, e);

                // The connection was closed/reset by the remote server, so we'll remove and close it
                match ws_streams.lock().await.get_mut(&(target.process.clone(), channel_id)) {
                    Some(ws_sink) => {
                        let _ = ws_sink.close().await;
                    },
                    None => {}
                }
                ws_streams.lock().await.remove(&(target.process.clone(), channel_id));

                handle_ws_message(
                    our.clone(),
                    id,
                    target.clone(),
                    WebSocketClientAction::Close {
                        channel_id,
                    },
                    None,
                    send_to_loop.clone(),
                ).await;

                break;
            }
        }
    };
}

async fn handle_http_request(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    json: Vec<u8>,
    body: Option<Payload>,
    client: reqwest::Client,
    send_to_loop: MessageSender,
    print_tx: PrintSender,
) {
    let req: OutgoingHttpRequest = match serde_json::from_slice(&json) {
        Ok(req) => req,
        Err(_e) => {
            make_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::BadRequest {
                    req: String::from_utf8(json).unwrap_or_default(),
                },
                send_to_loop,
            )
            .await;
            return;
        }
    };

    let Ok(req_method) = http::Method::from_bytes(req.method.as_bytes()) else {
        make_error_message(
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
                make_error_message(
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

    if let Some(payload) = body {
        request_builder = request_builder.body(payload.bytes);
    }

    let Ok(request) = request_builder
        .headers(deserialize_headers(req.headers))
        .build()
    else {
        make_error_message(
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
                    content: format!("http_client: executed request, got response"),
                })
                .await;
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our.to_string(),
                        process: ProcessId::new(Some("http_client"), "sys", "uqbar"),
                    },
                    target,
                    rsvp: None,
                    message: Message::Response((
                        Response {
                            inherit: false,
                            ipc: serde_json::to_vec::<Result<HttpResponse, HttpClientError>>(&Ok(
                                HttpResponse {
                                    status: response.status().as_u16(),
                                    headers: serialize_headers(response.headers()),
                                },
                            ))
                            .unwrap(),
                            metadata: None,
                        },
                        None,
                    )),
                    payload: Some(Payload {
                        mime: None,
                        bytes: response.bytes().await.unwrap_or_default().to_vec(),
                    }),
                    signed_capabilities: None,
                })
                .await;
        }
        Err(e) => {
            let _ = print_tx
                .send(Printout {
                    verbosity: 1,
                    content: format!("http_client: executed request but got error"),
                })
                .await;
            make_error_message(
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

async fn make_error_message(
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
                    process: ProcessId::new(Some("http_client"), "sys", "uqbar"),
                },
                target,
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        ipc: serde_json::to_vec::<Result<HttpResponse, HttpClientError>>(&Err(
                            error,
                        ))
                        .unwrap(),
                        metadata: None,
                    },
                    None,
                )),
                payload: None,
                signed_capabilities: None,
            })
            .await;
    }
}

async fn send_ws_push(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    channel_id: u32,
    message_type: WsMessageType,
    payload: Option<Payload>,
    ws_streams: Arc<Mutex<WebSocketMap>>,
    send_to_loop: MessageSender,
) {
    let mut streams = ws_streams.lock().await;
    let Some(ws_stream) = streams.get_mut(&(target.process.clone(), channel_id)) else {
        // send an error message to the target
        return;
    };

    let result = match message_type {
        WsMessageType::Text => {
            let Some(payload) = payload else {
                // send an error message to the target
                make_error_message(
                    our,
                    id,
                    target,
                    expects_response,
                    HttpClientError::BadRequest {
                        req: "no payload".into()
                    },
                    send_to_loop,
                ).await;
                return;
            };

            let Ok(text) = String::from_utf8(payload.bytes) else {
                // send an error message to the target
                make_error_message(
                    our,
                    id,
                    target,
                    expects_response,
                    HttpClientError::BadRequest {
                            req: "failed to convert payload to string".into()
                    },
                    send_to_loop,
                ).await;
                return;
            };

            ws_stream.send(TungsteniteMessage::Text(text)).await
        },
        WsMessageType::Binary => {
            let Some(payload) = payload else {
                // send an error message to the target
                make_error_message(
                    our,
                    id,
                    target,
                    expects_response,
                    HttpClientError::BadRequest {
                        req: "no payload".into()
                    },
                    send_to_loop,
                ).await;
                return;
            };

            ws_stream.send(TungsteniteMessage::Binary(payload.bytes)).await
        },
        WsMessageType::Ping => {
            // send a Request to the target with the ping as payload
            ws_stream.send(TungsteniteMessage::Ping(vec![])).await
        },
        WsMessageType::Pong => {
            // send a Request to the target with the pong as payload
            ws_stream.send(TungsteniteMessage::Pong(vec![])).await
        },
    };

    match result {
        Ok(_) => {},
        Err(_) => {
            // send an error message to the target
            make_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::RequestFailed {
                    error: "failed to send message".into()
                },
                send_to_loop,
            ).await;
        }
    }
}

async fn close_ws_connection(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    channel_id: u32,
    ws_streams: Arc<Mutex<WebSocketMap>>,
    send_to_loop: MessageSender,
) {
    let mut streams = ws_streams.lock().await;
    let Some(ws_sink) = streams.get_mut(&(target.process.clone(), channel_id)) else {
        // send an error message to the target
        make_error_message(
            our,
            id,
            target.clone(),
            expects_response,
            HttpClientError::BadRequest {
                req: format!("No open WebSocket matching {}, {}", target.process.to_string(), channel_id),
            },
            send_to_loop,
        ).await;
        return;
    };

    // Close the stream. The stream is closed even on error.
    match ws_sink.close().await {
        Ok(_) => {},
        Err(_) => {}
    }

    streams.remove(&(target.process, channel_id));
}

async fn handle_ws_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    action: WebSocketClientAction,
    payload: Option<Payload>,
    send_to_loop: MessageSender,
) {
    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: our.to_string(),
                process: ProcessId::new(Some("http_client"), "sys", "uqbar"),
            },
            target,
            rsvp: None,
            message: Message::Request(
                Request {
                    inherit: false,
                    ipc: serde_json::to_vec::<WebSocketClientAction>(&action)
                    .unwrap(),
                    expects_response: None,
                    metadata: None,
                }
            ),
            payload,
            signed_capabilities: None,
        })
        .await;
}
