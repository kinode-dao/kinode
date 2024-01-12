use crate::eth::types::*;
use crate::http::server_types::{HttpServerAction, HttpServerRequest, WsMessageType};
use crate::types::*;
use anyhow::Result;
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

/// The ETH provider runtime process is responsible for connecting to one or more ETH RPC providers
/// and using them to service indexing requests from other apps. This could also be done by a wasm
/// app, but in the future, this process will hopefully expand in scope to perform more complex
/// indexing and ETH node responsibilities.
pub async fn provider(
    our: String,
    rpc_url: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    // for now, we can only handle WebSocket RPC URLs. In the future, we should
    // be able to handle HTTP too, at least.
    match Url::parse(&rpc_url)?.scheme() {
        "http" | "https" => {
            return Err(anyhow::anyhow!("eth: http provider not supported yet!"));
        }
        "ws" | "wss" => {}
        _ => {
            return Err(anyhow::anyhow!("eth: provider must use http or ws!"));
        }
    }

    while let Some(km) = recv_in_client.recv().await {
        // this module only handles requests, ignores all responses
        let Message::Request(req) = &km.message else {
            continue;
        };
        let Ok(action) = serde_json::from_slice::<EthAction>(&req.body) else {
            continue;
        };
        match handle_request(&our, action, &send_to_loop).await {
            Ok(()) => {}
            Err(e) => {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("eth: error handling request: {:?}", e),
                    })
                    .await;
            }
        }
    }
    Err(anyhow::anyhow!("eth: fatal: message receiver closed!"))
}

async fn handle_request(
    our: &str,
    action: EthAction,
    send_to_loop: &MessageSender,
) -> Result<(), anyhow::Error> {
    match action {
        EthAction::SubscribeLogs(req) => {
            todo!()
        }
        EthAction::UnsubscribeLogs(channel_id) => {
            todo!()
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
    loop {
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
        ws_provider_subscription.subscription = Some(stream.id);

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
                        body: json!({
                            "EventSubscription": serde_json::to_value(event.clone()).unwrap()
                        })
                        .to_string()
                        .into_bytes(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await
                .unwrap();
        }
    }
}

async fn bind_websockets(our: &str, send_to_loop: &MessageSender) {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our.to_string(),
                process: ETH_PROCESS_ID.clone(),
            },
            target: Address {
                node: our.to_string(),
                process: HTTP_SERVER_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                body: serde_json::to_vec(&HttpServerAction::WebSocketBind {
                    path: "/".to_string(),
                    authenticated: false,
                    encrypted: false,
                })
                .unwrap(),
                metadata: None,
                expects_response: None,
                capabilities: vec![],
            }),
            lazy_load_blob: None,
        })
        .await;
}

async fn bootstrap_websocket_connections(
    our: &str,
    rpc_url: &str,
    connections: Arc<Mutex<RpcConnections>>,
    send_to_loop: &mut MessageSender,
) -> Result<()> {
    let our = our.to_string();
    let rpc_url = rpc_url.to_string();
    let send_to_loop = send_to_loop.clone();
    let connections = connections.clone();
    tokio::spawn(async move {
        loop {
            let Ok((ws_stream, _response)) = connect_async(&rpc_url).await else {
                println!(
                    "error! couldn't connect to eth_rpc provider: {:?}, trying again in 3s\r",
                    rpc_url
                );
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            };
            let (ws_sender, mut ws_receiver) = ws_stream.split();

            let mut connections_guard = connections.lock().await;
            connections_guard.ws_sender = Some(ws_sender);
            connections_guard.ws_provider = Some(Provider::<Ws>::connect(&rpc_url).await.unwrap());
            drop(connections_guard);

            handle_external_websocket_passthrough(
                &our,
                connections.clone(),
                &mut ws_receiver,
                &send_to_loop,
            )
            .await;
        }
    });
    Ok(())
}

async fn handle_external_websocket_passthrough(
    our: &str,
    connections: Arc<Mutex<RpcConnections>>,
    ws_receiver: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    send_to_loop: &MessageSender,
) {
    while let Some(message) = ws_receiver.next().await {
        match message {
            Ok(msg) => {
                if let Ok(text) = msg.into_text() {
                    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    let id = json["id"].as_u64().unwrap() as u32;
                    let channel_id: u32 = *connections.lock().await.ws_sender_ids.get(&id).unwrap();

                    json["id"] = serde_json::Value::from(id - channel_id);

                    let _ = send_to_loop
                        .send(KernelMessage {
                            id: rand::random(),
                            source: Address {
                                node: our.to_string(),
                                process: ETH_PROCESS_ID.clone(),
                            },
                            target: Address {
                                node: our.to_string(),
                                process: HTTP_SERVER_PROCESS_ID.clone(),
                            },
                            rsvp: None,
                            message: Message::Request(Request {
                                inherit: false,
                                body: serde_json::to_vec(&HttpServerAction::WebSocketPush {
                                    channel_id,
                                    message_type: WsMessageType::Text,
                                })
                                .unwrap(),
                                metadata: None,
                                expects_response: None,
                                capabilities: vec![],
                            }),
                            lazy_load_blob: Some(LazyLoadBlob {
                                bytes: json.to_string().as_bytes().to_vec(),
                                mime: None,
                            }),
                        })
                        .await;
                }
            }
            Err(_) => break,
        }
    }
}
