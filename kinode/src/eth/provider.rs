use alloy_providers::provider::Provider;
use alloy_pubsub::{PubSubFrontend, RawSubscription};
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::pubsub::SubscriptionResult;
use alloy_transport_ws::WsConnect;
use anyhow::Result;
use dashmap::DashMap;
use lib::types::core::*;
use lib::types::eth::*;
use std::str::FromStr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

/// The ETH provider runtime process is responsible for connecting to one or more ETH RPC providers
/// and using them to service indexing requests from other apps. This could also be done by a wasm
/// app, but in the future, this process will hopefully expand in scope to perform more complex
/// indexing and ETH node responsibilities.
pub async fn provider(
    our: String,
    rpc_url: Option<String>, // if None, bootstrap from router, can set settings later?
    public: bool,            // todo, whitelists etc.
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    _print_tx: PrintSender,
) -> Result<()> {
    let our = Arc::new(our);

    // Initialize the provider conditionally based on rpc_url
    // Todo: make provider<T> support multiple transports, one direct and another passthrough.
    let provider = if let Some(rpc_url) = rpc_url {
        // If rpc_url is Some, proceed with URL parsing and client setup
        match Url::parse(&rpc_url)?.scheme() {
            "http" | "https" => {
                return Err(anyhow::anyhow!(
                    "eth: you provided a `http(s)://` Ethereum RPC, but only `ws(s)://` is supported. Please try again with a `ws(s)://` provider"
                ));
            }
            "ws" | "wss" => {}
            s => {
                return Err(anyhow::anyhow!(
                    "eth: you provided a `{s:?}` Ethereum RPC, but only `ws(s)://` is supported. Please try again with a `ws(s)://` provider"
                ));
            }
        }

        let connector = WsConnect {
            url: rpc_url,
            auth: None,
        };

        let client = ClientBuilder::default().ws(connector).await?;
        Some(Provider::new_with_client(client))
    } else {
        None
    };

    let provider = Arc::new(provider);

    // handles of longrunning subscriptions.
    let connections: DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>> = DashMap::new();
    let connections = Arc::new(connections);

    // add whitelist, logic in provider middleware?
    let public = Arc::new(public);

    while let Some(km) = recv_in_client.recv().await {
        // clone Arcs
        let our = our.clone();
        let send_to_loop = send_to_loop.clone();
        let provider = provider.clone();
        let connections = connections.clone();
        let public = public.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_message(
                &our,
                &km,
                &send_to_loop,
                provider.clone(),
                connections.clone(),
                public.clone(),
            )
            .await
            {
                let _ = send_to_loop
                    .send(make_error_message(our.to_string(), km, e))
                    .await;
            };
        });
    }
    Err(anyhow::anyhow!("eth: fatal: message receiver closed!"))
}

async fn handle_message(
    our: &str,
    km: &KernelMessage,
    send_to_loop: &MessageSender,
    provider: Arc<Option<Provider<PubSubFrontend>>>,
    connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
    public: Arc<bool>,
) -> Result<(), EthError> {
    match &km.message {
        Message::Request(req) => {
            if km.source.node == our {
                if let Some(provider) = provider.as_ref() {
                    handle_local_request(our, km, send_to_loop, provider, connections, public)
                        .await?
                } else {
                    // we have no provider, let's send this request to someone who has one.
                    let request = KernelMessage {
                        id: km.id,
                        source: Address {
                            node: our.to_string(),
                            process: ETH_PROCESS_ID.clone(),
                        },
                        target: Address {
                            node: "jugodenaranja.os".to_string(),
                            process: ETH_PROCESS_ID.clone(),
                        },
                        rsvp: Some(km.source.clone()),
                        message: Message::Request(req.clone()),
                        lazy_load_blob: None,
                    };

                    let _ = send_to_loop.send(request).await;
                }
            } else {
                // either someone asking us for rpc, or we are passing through a sub event.
                handle_remote_request(our, km, send_to_loop, provider, connections, public).await?
            }
        }
        Message::Response(_) => {
            // handle passthrough responses, send to rsvp.
            if km.source.process == ProcessId::from_str("eth:distro:sys").unwrap() {
                if let Some(rsvp) = &km.rsvp {
                    let _ = send_to_loop
                        .send(KernelMessage {
                            id: rand::random(),
                            source: Address {
                                node: our.to_string(),
                                process: ETH_PROCESS_ID.clone(),
                            },
                            target: rsvp.clone(),
                            rsvp: None,
                            message: km.message.clone(),
                            lazy_load_blob: None,
                        })
                        .await;
                }
            }
        }
    }
    Ok(())
}

async fn handle_local_request(
    our: &str,
    km: &KernelMessage,
    send_to_loop: &MessageSender,
    provider: &Provider<PubSubFrontend>,
    connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
    public: Arc<bool>,
) -> Result<(), EthError> {
    let Message::Request(req) = &km.message else {
        return Err(EthError::InvalidMethod(
            "eth: only accepts requests".to_string(),
        ));
    };
    let action = serde_json::from_slice::<EthAction>(&req.body).map_err(|e| {
        EthError::InvalidMethod(format!("eth: failed to deserialize request: {:?}", e))
    })?;

    // we might want some of these in payloads.. sub items?
    let return_body: EthResponse = match action {
        EthAction::SubscribeLogs {
            sub_id,
            kind,
            params,
        } => {
            let sub_id = (km.target.process.clone(), sub_id);

            let kind = serde_json::to_value(&kind).unwrap();
            let params = serde_json::to_value(&params).unwrap();

            let id = provider
                .inner()
                .prepare("eth_subscribe", [kind, params])
                .await
                .map_err(|e| EthError::TransportError(e.to_string()))?;

            let rx = provider.inner().get_raw_subscription(id).await;
            let handle = tokio::spawn(handle_subscription_stream(
                our.to_string(),
                sub_id.1.clone(),
                rx,
                km.source.clone(),
                km.rsvp.clone(),
                send_to_loop.clone(),
            ));

            connections.insert(sub_id, handle);
            EthResponse::Ok
        }
        EthAction::UnsubscribeLogs(sub_id) => {
            let sub_id = (km.target.process.clone(), sub_id);
            let handle = connections
                .remove(&sub_id)
                .ok_or(EthError::SubscriptionNotFound)?;

            handle.1.abort();
            EthResponse::Ok
        }
        EthAction::Request { method, params } => {
            let method = to_static_str(&method).ok_or(EthError::InvalidMethod(method))?;

            let response: serde_json::Value = provider
                .inner()
                .prepare(method, params)
                .await
                .map_err(|e| EthError::TransportError(e.to_string()))?;
            println!("got a normal request! ");
            EthResponse::Response { value: response }
        }
    };

    let response = KernelMessage {
        id: km.id,
        source: Address {
            node: our.to_string(),
            process: ETH_PROCESS_ID.clone(),
        },
        target: km.source.clone(),
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                body: serde_json::to_vec(&return_body).unwrap(),
                metadata: req.metadata.clone(),
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: None,
    };

    let _ = send_to_loop.send(response).await;

    Ok(())
}

// here we are either processing another nodes request.
// or we are passing through an ethSub Request..
async fn handle_remote_request(
    our: &str,
    km: &KernelMessage,
    send_to_loop: &MessageSender,
    provider: Arc<Option<Provider<PubSubFrontend>>>,
    connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
    public: Arc<bool>,
) -> Result<(), EthError> {
    let Message::Request(req) = &km.message else {
        return Err(EthError::InvalidMethod(
            "eth: only accepts requests".to_string(),
        ));
    };

    if let Some(provider) = provider.as_ref() {
        // we need some sort of agreement perhaps on rpc providing.
        // even with an agreement, fake ethsubevents could be sent to us.
        // light clients could verify blocks perhaps...
        if !*public {
            return Err(EthError::PermissionDenied("not on the list.".to_string()));
        }

        let action = serde_json::from_slice::<EthAction>(&req.body).map_err(|e| {
            EthError::InvalidMethod(format!("eth: failed to deserialize request: {:?}", e))
        })?;

        let return_body: EthResponse = match action {
            EthAction::SubscribeLogs {
                sub_id,
                kind,
                params,
            } => {
                let sub_id = (km.target.process.clone(), sub_id);

                let kind = serde_json::to_value(&kind).unwrap();
                let params = serde_json::to_value(&params).unwrap();

                let id = provider
                    .inner()
                    .prepare("eth_subscribe", [kind, params])
                    .await
                    .map_err(|e| EthError::TransportError(e.to_string()))?;

                let rx = provider.inner().get_raw_subscription(id).await;
                let handle = tokio::spawn(handle_subscription_stream(
                    our.to_string(),
                    sub_id.1.clone(),
                    rx,
                    km.target.clone(),
                    km.rsvp.clone(),
                    send_to_loop.clone(),
                ));

                connections.insert(sub_id, handle);
                EthResponse::Ok
            }
            EthAction::UnsubscribeLogs(sub_id) => {
                let sub_id = (km.target.process.clone(), sub_id);
                let handle = connections
                    .remove(&sub_id)
                    .ok_or(EthError::SubscriptionNotFound)?;

                handle.1.abort();
                EthResponse::Ok
            }
            EthAction::Request { method, params } => {
                let method = to_static_str(&method).ok_or(EthError::InvalidMethod(method))?;

                let response: serde_json::Value = provider
                    .inner()
                    .prepare(method, params)
                    .await
                    .map_err(|e| EthError::TransportError(e.to_string()))?;

                EthResponse::Response { value: response }
            }
        };

        let response = KernelMessage {
            id: km.id,
            source: Address {
                node: our.to_string(),
                process: ETH_PROCESS_ID.clone(),
            },
            target: km.source.clone(),
            rsvp: km.rsvp.clone(),
            message: Message::Response((
                Response {
                    inherit: false,
                    body: serde_json::to_vec(&return_body).unwrap(),
                    metadata: req.metadata.clone(),
                    capabilities: vec![],
                },
                None,
            )),
            lazy_load_blob: None,
        };

        let _ = send_to_loop.send(response).await;
    } else {
        // We do not have a provider, this is a reply for a request made by us.
        if let Ok(eth_sub) = serde_json::from_slice::<EthSub>(&req.body) {
            // forward...
            if let Some(target) = km.rsvp.clone() {
                let _ = send_to_loop
                    .send(KernelMessage {
                        id: rand::random(),
                        source: Address {
                            node: our.to_string(),
                            process: ETH_PROCESS_ID.clone(),
                        },
                        target: target,
                        rsvp: None,
                        message: Message::Request(req.clone()),
                        lazy_load_blob: None,
                    })
                    .await;
            }
        }
    }
    Ok(())
}

/// Executed as a long-lived task. The JoinHandle is stored in the `connections` map.
/// This task is responsible for connecting to the ETH RPC provider and streaming logs
/// for a specific subscription made by a process.
async fn handle_subscription_stream(
    our: String,
    sub_id: u64,
    mut rx: RawSubscription,
    target: Address,
    rsvp: Option<Address>,
    send_to_loop: MessageSender,
) -> Result<(), EthError> {
    match rx.recv().await {
        Err(e) => {
            return Err(EthError::SubscriptionClosed(sub_id))?;
        }
        Ok(value) => {
            // this should not return in case of one failed event?
            let event: SubscriptionResult = serde_json::from_str(value.get()).map_err(|_| {
                EthError::RpcError("eth: failed to deserialize subscription result".to_string())
            })?;
            send_to_loop
                .send(KernelMessage {
                    id: rand::random(),
                    source: Address {
                        node: our,
                        process: ETH_PROCESS_ID.clone(),
                    },
                    target: target.clone(),
                    rsvp: rsvp.clone(),
                    message: Message::Request(Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&EthSub {
                            id: sub_id,
                            result: event,
                        })
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await
                .unwrap();
        }
    }
    Err(EthError::SubscriptionClosed(sub_id))
}

// todo, always send errors or no? general runtime question for other modules too.
fn make_error_message(our_node: String, km: KernelMessage, error: EthError) -> KernelMessage {
    let source = km.rsvp.unwrap_or_else(|| Address {
        node: our_node.clone(),
        process: km.source.process.clone(),
    });
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_node,
            process: ETH_PROCESS_ID.clone(),
        },
        target: source,
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                body: serde_json::to_vec(&EthResponse::Err(error)).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: None,
    }
}
