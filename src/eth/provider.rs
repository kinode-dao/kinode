use crate::eth::types::*;
use crate::http::types::{HttpServerAction, HttpServerRequest, WsMessageType};
use crate::types::*;
use anyhow::Result;
use dashmap::DashMap;
use ethers::prelude::Provider;
use ethers_providers::{Http, StreamExt, Ws};
use futures::stream::SplitSink;
use futures::SinkExt;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use url::Url;

struct Connections {
    ws_sender: Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TungsteniteMessage>>,
    ws_provider: Option<Provider<Ws>>,
    http_provider: Option<Provider<Http>>,
    uq_provider: Option<NodeId>,
}

// I need a data structure that tracks incoming requests from a particular websocket channel
// and associates the response from the response to the outgoing websocket message with that
// channel. It should then return the response to that channel.

// this should just map responses from the outgoing websocket request
// to the requests that made them

// Channel IDs to Nonces used to make unique IDs
type WsRequestNonces = Arc<DashMap<u32, u32>>;
// Request IDs to Channel IDs
type WsRequestIds = Arc<DashMap<u32, u32>>;

pub async fn provider(
    our: String,
    rpc_url: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    println!("eth_rpc: starting");

    let open_ws = KernelMessage {
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
    };

    let _ = send_to_loop.send(open_ws).await;

    let mut connections = Connections {
        ws_sender: None,
        ws_provider: None,
        http_provider: None,
        uq_provider: None,
    };

    let ws_request_nonces: WsRequestNonces = Arc::new(DashMap::new());
    let ws_request_ids: WsRequestIds = Arc::new(DashMap::new());

    match Url::parse(&rpc_url).unwrap().scheme() {
        "http" | "https" => {
            unreachable!()
        }
        "ws" | "wss" => {
            let (_ws_stream, _) = connect_async(&rpc_url).await.expect("failed to connect");
            let (_ws_sender, mut ws_receiver) = _ws_stream.split();

            connections.ws_sender = Some(_ws_sender);
            connections.ws_provider = Some(Provider::<Ws>::connect(rpc_url.clone()).await?);

            let ws_request_ids = ws_request_ids.clone();

            tokio::spawn(async move {
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
                                            ipc: serde_json::to_vec(
                                                &HttpServerAction::WebSocketPush {
                                                    channel_id: channel_id,
                                                    message_type: WsMessageType::Text,
                                                },
                                            )
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
                            } else {
                                println!("Received a binary message: {:?}", msg.into_data());
                            }
                        }
                        Err(e) => {
                            println!("Error receiving a message: {:?}", e);
                        }
                    }
                }
                Ok::<(), ()>(())
            });
        }
        _ => {
            unreachable!()
        }
    }

    while let Some(km) = recv_in_client.recv().await {
        match km.message {
            Message::Request(Request { ref ipc, .. }) => {
                println!("eth request");
                let _ = handle_request(
                    ipc,
                    km.source,
                    km.payload,
                    ws_request_nonces.clone(),
                    ws_request_ids.clone(),
                    &mut connections,
                )
                .await;
            }
            Message::Response((Response { ref ipc, .. }, ..)) => {
                println!("eth response");
                handle_response(ipc)?;
            }
            Message::Response(_) => todo!(),
            _ => {}
        }

        continue;
    }

    Ok(())
}

async fn handle_request(
    ipc: &Vec<u8>,
    source: Address,
    payload: Option<Payload>,
    ws_request_nonces: WsRequestNonces,
    ws_request_ids: WsRequestIds,
    connections: &mut Connections,
) -> Result<()> {
    println!("request");

    if let Ok(action) = serde_json::from_slice::<HttpServerRequest>(ipc) {
        match action {
            HttpServerRequest::WebSocketOpen { path, channel_id } => {
                println!("open {:?}, {:?}", path, channel_id);
            }
            HttpServerRequest::WebSocketPush {
                channel_id,
                message_type,
            } => match message_type {
                WsMessageType::Text => {
                    println!("text");

                    let bytes = payload.unwrap().bytes;
                    let text = std::str::from_utf8(&bytes).unwrap();
                    let mut json: serde_json::Value = serde_json::from_str(text)?;
                    let mut id = json["id"].as_u64().unwrap();

                    let mut nonce = ws_request_nonces.entry(channel_id).or_insert(0);

                    id += channel_id as u64;
                    id += *nonce as u64;
                    *nonce += 1;

                    ws_request_ids.insert(id as u32, channel_id);

                    json["id"] = serde_json::Value::from(id);

                    let _new_text = json.to_string();

                    let _ = connections
                        .ws_sender
                        .as_mut()
                        .unwrap()
                        .send(TungsteniteMessage::Text(_new_text))
                        .await;
                }
                WsMessageType::Binary => {
                    println!("binary");
                }
                WsMessageType::Ping => {
                    println!("ping");
                }
                WsMessageType::Pong => {
                    println!("pong");
                }
                WsMessageType::Close => {
                    println!("close");
                }
            },
            HttpServerRequest::WebSocketClose(channel_id) => {}
            HttpServerRequest::Http(_) => todo!(),
        }
    } else if let Ok(action) = serde_json::from_slice::<EthRpcAction>(ipc) {
        match action {
            EthRpcAction::JsonRpcRequest(_) => unreachable!(),
            EthRpcAction::Eth(method) => {}
            EthRpcAction::Debug(method) => {}
            EthRpcAction::Net(method) => {}
            EthRpcAction::Trace(method) => {}
            EthRpcAction::TxPool(method) => {}
        }
    } else {
        println!("unknown request");
    }

    Ok(())
}

fn handle_http() {}

fn handle_response(ipc: &Vec<u8>) -> Result<()> {
    let Ok(message) = serde_json::from_slice::<HttpServerAction>(ipc) else {
        return Ok(());
    };

    println!("response message {:?}", message);

    Ok(())
}
