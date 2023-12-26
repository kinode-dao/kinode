use crate::eth::types::{EthRpcAction, ProviderAction};
use crate::http::types::{HttpServerAction, HttpServerRequest, WsMessageType};
use crate::types::*;
use anyhow::Result;
use ethers::core::types::Filter;
use ethers::prelude::Provider;
use ethers::types::{ValueOrArray, U256, U64};
use ethers_providers::{Http, Middleware, StreamExt, Ws};
use futures::stream::SplitSink;
use futures::SinkExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
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

    send_to_loop.send(open_ws).await;

    let mut connections = Connections {
        ws_sender: None,
        ws_provider: None,
        http_provider: None,
        uq_provider: None,
    };

    match Url::parse(&rpc_url).unwrap().scheme() {
        "http" | "https" => {
            unreachable!()
        }
        "ws" | "wss" => {
            let (_ws_stream, _) = connect_async(&rpc_url).await.expect("failed to connect");
            let (_ws_sender, mut ws_receiver) = _ws_stream.split();

            connections.ws_sender = Some(_ws_sender);
            connections.ws_provider = Some(Provider::<Ws>::connect(rpc_url.clone()).await?);

            tokio::spawn(async move {
                while let Some(message) = ws_receiver.next().await {
                    match message {
                        Ok(msg) => {
                            if (msg.is_text()) {
                                println!("Received a text message: {}", msg.into_text().unwrap());
                            } else {
                                println!("Received a binary message: {:?}", msg.into_data());
                            }
                        }
                        Err(e) => {
                            println!("Error receiving a message: {:?}", e);
                        }
                    }
                }
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
                handle_request(ipc, km.payload, &mut connections).await;
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
    payload: Option<Payload>,
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

                    println!("{:?}", text);

                    connections
                        .ws_sender
                        .as_mut()
                        .unwrap()
                        .send(TungsteniteMessage::Text(text.to_string()))
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
