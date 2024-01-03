use crate::eth::types::*;
use crate::http::types::{HttpServerAction, HttpServerRequest, WsMessageType};
use crate::types::*;
use anyhow::Result;
use dashmap::DashMap;
use ethers::prelude::Provider;
use ethers_providers::{Middleware, StreamExt, Ws};
use futures::stream::SplitStream;
use futures::SinkExt;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

// Request IDs to Channel IDs
type WsRequestIds = Arc<DashMap<u32, u32>>;

pub async fn provider(
    our: String,
    rpc_url: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    _print_tx: PrintSender,
) -> Result<()> {
    println!("eth: starting");

    bind_websockets(&our, &send_to_loop).await;

    let connections = Arc::new(Mutex::new(RpcConnections::default()));

    match Url::parse(&rpc_url).unwrap().scheme() {
        "http" | "https" => {
            unreachable!()
        }
        "ws" | "wss" => {
            bootstrap_websocket_connections(
                our.clone(),
                rpc_url.clone(),
                connections.clone(),
                send_to_loop.clone(),
            )
            .await?;
        }
        _ => {
            unreachable!()
        }
    }

    while let Some(km) = recv_in_client.recv().await {
        match &km.message {
            Message::Request(req) => {
                let _ = handle_request(
                    our.clone(),
                    &km,
                    &req,
                    connections.clone(),
                    send_to_loop.clone(),
                )
                .await;
            }
            Message::Response((Response { ref ipc, .. }, ..)) => {
                handle_response(ipc)?;
            }
        }

        continue;
    }

    Ok(())
}

async fn handle_request(
    our: String,
    km: &KernelMessage,
    req: &Request,
    connections: Arc<Mutex<RpcConnections>>,
    send_to_loop: MessageSender,
) -> Result<()> {
    if let Ok(action) = serde_json::from_slice::<HttpServerRequest>(&req.ipc) {
        let _ = handle_http_server_request(action, km, connections).await;
    } else if let Ok(action) = serde_json::from_slice::<EthRequest>(&req.ipc) {
        let _ = handle_eth_request(action, our.clone(), km, connections, send_to_loop).await;
    } else {
        println!("unknown request");
    }

    Ok(())
}

async fn handle_http_server_request(
    action: HttpServerRequest,
    km: &KernelMessage,
    connections: Arc<Mutex<RpcConnections>>,
) -> Result<(), anyhow::Error> {
    match action {
        HttpServerRequest::WebSocketOpen { .. /*path, channel_id*/ } => {}
        HttpServerRequest::WebSocketPush {
            channel_id,
            message_type,
        } => match message_type {
            WsMessageType::Text => {
                let bytes = &km.payload.as_ref().unwrap().bytes;
                let text = std::str::from_utf8(&bytes).unwrap();
                let mut json: serde_json::Value = serde_json::from_str(text)?;
                let mut id = json["id"].as_u64().unwrap();

                id += channel_id as u64;

                json["id"] = serde_json::Value::from(id);

                let _new_text = json.to_string();

                let mut connections_guard = connections.lock().await;

                connections_guard
                    .ws_sender_ids
                    .insert(id as u32, channel_id);

                if let Some(ws_sender) = &mut connections_guard.ws_sender {
                    let _ = ws_sender.send(TungsteniteMessage::Text(_new_text)).await;
                }
            }
            WsMessageType::Binary => {}
            WsMessageType::Ping => {}
            WsMessageType::Pong => {}
            WsMessageType::Close => {}
        },
        HttpServerRequest::WebSocketClose(_channel_id) => {}
        HttpServerRequest::Http(_) => todo!(),
    }

    Ok(())
}

async fn handle_eth_request(
    action: EthRequest,
    our: String,
    km: &KernelMessage,
    connections: Arc<Mutex<RpcConnections>>,
    send_to_loop: MessageSender,
) -> Result<(), anyhow::Error> {
    match action {
        EthRequest::SubscribeLogs(req) => {
            let handle = tokio::spawn(spawn_provider_read_stream(
                our.clone(),
                req,
                km.clone(),
                connections.clone(),
                send_to_loop.clone(),
            ));

            let mut connections_guard = connections.lock().await;

            let ws_provider_subscription = connections_guard
                .ws_provider_subscriptions
                .entry(km.id)
                .or_insert(WsProviderSubscription::default());

            ws_provider_subscription.handle = Some(handle);

            drop(connections_guard);
        }
        EthRequest::UnsubscribeLogs(channel_id) => {
            let connections_guard = connections.lock().await;
            let ws_provider_stream = connections_guard
                .ws_provider_subscriptions
                .get(&channel_id)
                .unwrap();
            ws_provider_stream.kill().await;
        }
    }
    Ok(())
}

async fn spawn_provider_read_stream(
    our: String,
    req: SubscribeLogs,
    km: KernelMessage,
    connections: Arc<Mutex<RpcConnections>>,
    send_to_loop: MessageSender,
) {
    let mut connections_guard = connections.lock().await;

    let Some(ref ws_rpc_url) = connections_guard.ws_rpc_url else {
        todo!()
    };
    let ws_provider = match Provider::<Ws>::connect(&ws_rpc_url).await {
        Ok(provider) => provider,
        Err(e) => {
            println!("error connecting to ws provider: {:?}", e);
            return;
        }
    };

    let mut stream = match ws_provider.subscribe_logs(&req.filter.clone()).await {
        Ok(s) => s,
        Err(e) => {
            println!("error subscribing to logs: {:?}", e);
            return;
        }
    };

    let ws_provider_subscription = connections_guard
        .ws_provider_subscriptions
        .entry(km.id)
        .or_insert(WsProviderSubscription::default());

    ws_provider_subscription.provider = Some(ws_provider.clone());
    ws_provider_subscription.subscription = Some(stream.id.clone());

    drop(connections_guard);

    while let Some(event) = stream.next().await {
        send_to_loop
            .send(KernelMessage {
                id: rand::random(),
                source: Address {
                    node: our.clone(),
                    process: ETH_PROCESS_ID.clone(),
                },
                target: Address {
                    node: our.clone(),
                    process: km.source.process.clone(),
                },
                rsvp: None,
                message: Message::Request(Request {
                    inherit: false,
                    expects_response: None,
                    ipc: json!({
                        "EventSubscription": serde_json::to_value(event.clone()).unwrap()
                    })
                    .to_string()
                    .into_bytes(),
                    metadata: None,
                }),
                payload: None,
                signed_capabilities: None,
            })
            .await
            .unwrap();
    }
}

fn handle_response(ipc: &Vec<u8>) -> Result<()> {
    let Ok(_message) = serde_json::from_slice::<HttpServerAction>(ipc) else {
        return Ok(());
    };

    Ok(())
}

async fn bind_websockets(our: &String, send_to_loop: &MessageSender) {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our.clone(),
                process: ETH_PROCESS_ID.clone(),
            },
            target: Address {
                node: our.clone(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                ipc: serde_json::to_vec(&HttpServerAction::WebSocketBind {
                    path: "/".to_string(),
                    authenticated: false,
                    encrypted: false,
                })
                .unwrap(),
                metadata: None,
                expects_response: None,
            }),
            payload: None,
            signed_capabilities: None,
        })
        .await;
}

async fn bootstrap_websocket_connections(
    our: String,
    rpc_url: String,
    connections: Arc<Mutex<RpcConnections>>,
    send_to_loop: MessageSender,
) -> Result<()> {
    let (_ws_stream, status) = connect_async(&rpc_url).await.expect("failed to connect");
    let (_ws_sender, mut ws_receiver) = _ws_stream.split();

    let mut connections_guard = connections.lock().await;
    connections_guard.ws_sender = Some(_ws_sender);
    connections_guard.ws_provider = Some(Provider::<Ws>::connect(rpc_url.clone()).await?);
    connections_guard.ws_rpc_url = Some(rpc_url.clone());

    let our = our.clone();
    let ws_sender_ids = connections_guard.ws_sender_ids.clone();
    let send_to_loop = send_to_loop.clone();

    tokio::spawn(async move {
        handle_external_websocket_passthrough(
            our.clone(),
            ws_sender_ids.clone(),
            &mut ws_receiver,
            send_to_loop.clone(),
        )
        .await;
        Ok::<(), ()>(())
    });
    Ok(())
}

async fn handle_external_websocket_passthrough(
    our: String,
    ws_request_ids: WsRequestIds,
    ws_receiver: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    send_to_loop: MessageSender,
) {
    while let Some(message) = ws_receiver.next().await {
        match message {
            Ok(msg) => {
                if msg.is_text() {
                    let Ok(text) = msg.into_text() else {
                        todo!();
                    };
                    let json_result: Result<serde_json::Value, serde_json::Error> =
                        serde_json::from_str(&text);
                    let Ok(mut _json) = json_result else {
                        todo!();
                    };
                    let id = _json["id"].as_u64().unwrap() as u32;
                    let channel_id = ws_request_ids.get(&id).unwrap().clone();

                    _json["id"] = serde_json::Value::from(id - channel_id);

                    let _ = send_to_loop
                        .send(KernelMessage {
                            id: rand::random(),
                            source: Address {
                                node: our.clone(),
                                process: ETH_PROCESS_ID.clone(),
                            },
                            target: Address {
                                node: our.clone(),
                                process: HTTP_SERVER_PROCESS_ID.clone(),
                            },
                            rsvp: None,
                            message: Message::Request(Request {
                                inherit: false,
                                ipc: serde_json::to_vec(&HttpServerAction::WebSocketPush {
                                    channel_id: channel_id,
                                    message_type: WsMessageType::Text,
                                })
                                .unwrap(),
                                metadata: None,
                                expects_response: None,
                            }),
                            payload: Some(Payload {
                                bytes: _json.to_string().as_bytes().to_vec(),
                                mime: None,
                            }),
                            signed_capabilities: None,
                        })
                        .await;
                } // TODO: an rpc request may come as binary so may need to handle
            }
            Err(e) => {
                panic!("eth: passthrough websocket error {:?}", e);
            }
        }
    }
}
