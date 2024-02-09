use alloy_pubsub::{PubSubFrontend, RawSubscription};
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::pubsub::SubscriptionResult;
use alloy_transport_ws::WsConnect;
use anyhow::Result;
use dashmap::DashMap;
use lib::types::core::*;
use lib::types::eth::*;
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
        Some(alloy_providers::provider::Provider::new_with_client(client))
    } else {
        None
    };

    let provider = Arc::new(provider);

    // handles of longrunning subscriptions.
    let connections: DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>> = DashMap::new();
    let connections = Arc::new(connections);

    // passthrough responses
    let responses: DashMap<u64, (u64, ProcessId)> = DashMap::new();
    let responses = Arc::new(responses);

    // add whitelist, logic in provider middleware?
    let public = Arc::new(public);

    while let Some(km) = recv_in_client.recv().await {
        // clone Arcs
        let our = our.clone();
        let send_to_loop = send_to_loop.clone();
        let provider = provider.clone();
        let connections = connections.clone();
        let responses = responses.clone();
        let public = public.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_request(
                &our,
                &km,
                &send_to_loop,
                provider.clone(),
                connections.clone(),
                responses.clone(),
                public.clone(),
            )
            .await
            {
                println!("got error: {:?}", e);
            }
        });
    }
    Err(anyhow::anyhow!("eth: fatal: message receiver closed!"))
}

async fn handle_request(
    our: &str,
    km: &KernelMessage,
    send_to_loop: &MessageSender,
    provider: Arc<Option<alloy_providers::provider::Provider<PubSubFrontend>>>,
    connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
    responses: Arc<DashMap<u64, (u64, ProcessId)>>,
    public: Arc<bool>,
) -> Result<(), EthError> {
    let Message::Request(req) = &km.message else {
        return Err(EthError::ProviderError(
            "eth: only accepts requests".to_string(),
        ));
    };

    if let Some(provider) = provider.as_ref() {
        let action = serde_json::from_slice::<EthAction>(&req.body).map_err(|e| {
            EthError::ProviderError(format!("eth: failed to deserialize request: {:?}", e))
        })?;

        if !*public && km.source.node != our {
            return Err(EthError::ProviderError(
                "eth: only accepts requests from apps".to_string(),
            ));
        }

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
                    .unwrap();

                let target = km.rsvp.clone().unwrap_or_else(|| Address {
                    node: our.to_string(),
                    process: km.source.process.clone(),
                });

                let rx = provider.inner().get_raw_subscription(id).await;
                let handle = tokio::spawn(handle_subscription_stream(
                    our.to_string(),
                    sub_id.1.clone(),
                    rx,
                    target,
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
                let method = to_static_str(&method).ok_or(EthError::ProviderError(format!(
                    "eth: method not found: {}",
                    method
                )))?;

                // throw transportErrorKinds straight back to process
                let response: serde_json::Value =
                    provider.inner().prepare(method, params).await.unwrap();

                EthResponse::Request(response)
            }
        };

        // todo: fix km.clone() and metadata.clone()
        if let Some(target) = km.clone().rsvp.or_else(|| {
            req.expects_response.map(|_| Address {
                node: our.to_string(),
                process: km.source.process.clone(),
            })
        }) {
            let response = KernelMessage {
                id: km.id,
                source: Address {
                    node: our.to_string(),
                    process: ETH_PROCESS_ID.clone(),
                },
                target: target.clone(),
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

            // Send the response, handling potential errors appropriately
            let _ = send_to_loop.send(response).await;
        };
    } else {
        // passthrough
        // if node == our, forward to provider
        // hoping that rsvp can fix the rest.
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
    send_to_loop: MessageSender,
) -> Result<(), EthError> {
    match rx.recv().await {
        Err(e) => {
            println!("got an error from the subscription stream: {:?}", e);
            // TODO should we stop the subscription here?
            // return Err(EthError::ProviderError(format!("{:?}", e)));
        }
        Ok(value) => {
            let event: SubscriptionResult = serde_json::from_str(value.get())
                .map_err(|e| EthError::ProviderError(format!("{:?}", e)))?;
            send_to_loop
                .send(KernelMessage {
                    id: rand::random(),
                    source: Address {
                        node: our,
                        process: ETH_PROCESS_ID.clone(),
                    },
                    target: target.clone(),
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: false,
                        expects_response: None,
                        body: serde_json::to_vec(&EthResponse::Sub {
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
    Err(EthError::SubscriptionClosed)
}
