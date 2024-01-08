use crate::eth::types::*;
use crate::http::types::{HttpServerAction, HttpServerRequest, WsMessageType};
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

pub async fn provider(
    our: String,
    rpc_url: String,
    mut send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    bind_websockets(&our, &send_to_loop).await;
    let mut connections = RpcConnections::default();

    match Url::parse(&rpc_url).unwrap().scheme() {
        "http" | "https" => {
            return Err(anyhow::anyhow!("eth: http provider not supported yet!"));
        }
        "ws" | "wss" => {
            bootstrap_websocket_connections(&our, &rpc_url, &mut connections, &mut send_to_loop)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "eth: error bootstrapping websocket connections to {}: {:?}",
                        rpc_url,
                        e
                    )
                })?;
        }
        _ => {
            return Err(anyhow::anyhow!("eth: provider must use http or ws!"));
        }
    }

    let connections = Arc::new(Mutex::new(connections));

    while let Some(km) = recv_in_client.recv().await {
        match &km.message {
            Message::Request(req) => {
                match handle_request(&our, &km, &req, &connections, &send_to_loop).await {
                    Ok(()) => {}
                    Err(e) => {
                        let _ = print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("eth: error handling request: {:?}", e),
                            })
                            .await;
                    }
                }
            }
            _ => {}
        }
    }
    return Err(anyhow::anyhow!("eth: fatal: message receiver closed!"));
}

async fn handle_request(
    our: &str,
    km: &KernelMessage,
    req: &Request,
    connections: &Arc<Mutex<RpcConnections>>,
    send_to_loop: &MessageSender,
) -> Result<()> {
    if let Ok(action) = serde_json::from_slice::<HttpServerRequest>(&req.body) {
        return handle_http_server_request(action, km, connections).await;
    } else if let Ok(action) = serde_json::from_slice::<EthRequest>(&req.body) {
        return handle_eth_request(action, our, km, connections, send_to_loop).await;
    } else {
        return Err(anyhow::anyhow!("malformed request"));
    }
}

async fn handle_http_server_request(
    action: HttpServerRequest,
    km: &KernelMessage,
    connections: &Arc<Mutex<RpcConnections>>,
) -> Result<(), anyhow::Error> {
    if let HttpServerRequest::WebSocketPush {
        channel_id,
        message_type,
    } = action
    {
        if message_type == WsMessageType::Text {
            let bytes = &km.lazy_load_blob.as_ref().unwrap().bytes;
            let text = std::str::from_utf8(&bytes).unwrap();
            let mut json: serde_json::Value = serde_json::from_str(text)?;
            let mut id = json["id"].as_u64().unwrap();

            id += channel_id as u64;

            json["id"] = serde_json::Value::from(id);

            let new_text = json.to_string();

            let mut connections_guard = connections.lock().await;
            connections_guard
                .ws_sender_ids
                .insert(id as u32, channel_id);
            if let Some(ws_sender) = &mut connections_guard.ws_sender {
                let _ = ws_sender.send(TungsteniteMessage::Text(new_text)).await;
            }
        }
    }
    Ok(())
}

async fn handle_eth_request(
    action: EthRequest,
    our: &str,
    km: &KernelMessage,
    connections: &Arc<Mutex<RpcConnections>>,
    send_to_loop: &MessageSender,
) -> Result<(), anyhow::Error> {
    match action {
        EthRequest::SubscribeLogs(req) => {
            let handle = tokio::spawn(spawn_provider_read_stream(
                our.to_string(),
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
            let mut connections_guard = connections.lock().await;
            if let Some(ws_provider_subscription) = connections_guard
                .ws_provider_subscriptions
                .remove(&channel_id)
            {
                ws_provider_subscription.kill().await;
            }
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
    connections: &mut RpcConnections,
    send_to_loop: &mut MessageSender,
) -> Result<()> {
    let (ws_stream, _response) = connect_async(rpc_url).await.map_err(|e| {
        anyhow::anyhow!(
            "eth: error connecting to websocket provider at {}: {:?}",
            rpc_url,
            e
        )
    })?;
    let (ws_sender, ws_receiver) = ws_stream.split();

    connections.ws_sender = Some(ws_sender);
    connections.ws_rpc_url = Some(rpc_url.to_string());
    connections.ws_provider = Some(Provider::<Ws>::connect(rpc_url).await?);

    tokio::spawn(handle_external_websocket_passthrough(
        our.to_string(),
        connections.ws_sender_ids.clone(),
        ws_receiver,
        send_to_loop.clone(),
    ));
    Ok(())
}

async fn handle_external_websocket_passthrough(
    our: String,
    ws_request_ids: WsRequestIds,
    mut ws_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    send_to_loop: MessageSender,
) {
    while let Some(message) = ws_receiver.next().await {
        match message {
            Ok(msg) => {
                if let Ok(text) = msg.into_text() {
                    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    let id = json["id"].as_u64().unwrap() as u32;
                    let channel_id = ws_request_ids.get(&id).unwrap().clone();

                    json["id"] = serde_json::Value::from(id - channel_id);

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
