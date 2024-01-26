use crate::eth::types::*;
use crate::types::*;
use anyhow::Result;
use ethers::prelude::Provider;
use ethers::types::Filter;
use ethers_providers::{Middleware, StreamExt, Ws};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

const WS_RECONNECTS: usize = 10_000; // TODO workshop this

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
    let our = Arc::new(our);
    // for now, we can only handle WebSocket RPC URLs. In the future, we should
    // be able to handle HTTP too, at least.
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

    let provider = match Provider::<Ws>::connect_with_reconnects(&rpc_url, WS_RECONNECTS).await {
        Ok(provider) => provider,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "eth: fatal: given RPC URL could not connect! {e:?}"
            ));
        }
    };

    let mut connections = RpcConnections {
        provider,
        ws_provider_subscriptions: HashMap::new(),
    };

    while let Some(km) = recv_in_client.recv().await {
        // this module only handles requests, ignores all responses
        let Message::Request(req) = &km.message else {
            continue;
        };
        let Ok(action) = serde_json::from_slice::<EthAction>(&req.body) else {
            continue;
        };
        match handle_request(
            our.clone(),
            &km.rsvp.unwrap_or(km.source.clone()),
            action,
            &mut connections,
            &send_to_loop,
        )
        .await
        {
            Ok(()) => {}
            Err(e) => {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("eth: error handling request: {:?}", e),
                    })
                    .await;
                if req.expects_response.is_some() {
                    send_to_loop
                        .send(KernelMessage {
                            id: km.id,
                            source: Address {
                                node: our.to_string(),
                                process: ETH_PROCESS_ID.clone(),
                            },
                            target: Address {
                                node: our.to_string(),
                                process: km.source.process.clone(),
                            },
                            rsvp: None,
                            message: Message::Response((
                                Response {
                                    inherit: false,
                                    body: serde_json::to_vec::<Result<(), EthError>>(&Err(e))?,
                                    metadata: None,
                                    capabilities: vec![],
                                },
                                None,
                            )),
                            lazy_load_blob: None,
                        })
                        .await?;
                }
            }
        }
    }
    Err(anyhow::anyhow!("eth: fatal: message receiver closed!"))
}

async fn handle_request(
    our: Arc<String>,
    target: &Address,
    action: EthAction,
    connections: &mut RpcConnections,
    send_to_loop: &MessageSender,
) -> Result<(), EthError> {
    match action {
        EthAction::SubscribeLogs { sub_id, filter } => {
            let sub_id = (target.process.clone(), sub_id);

            // if this process has already used this subscription ID,
            // this subscription will **overwrite** the existing one.

            let handle = tokio::spawn(handle_subscription_stream(
                our.clone(),
                connections.provider.clone(),
                filter,
                target.clone(),
                send_to_loop.clone(),
            ));
            connections.ws_provider_subscriptions.insert(sub_id, handle);
            Ok(())
        }
        EthAction::UnsubscribeLogs(sub_id) => {
            let sub_id = (target.process.clone(), sub_id);
            let handle = connections
                .ws_provider_subscriptions
                .remove(&sub_id)
                .ok_or(EthError::SubscriptionNotFound)?;

            handle.abort();
            Ok(())
        }
    }
}

/// Executed as a long-lived task. The JoinHandle is stored in the `connections` map.
/// This task is responsible for connecting to the ETH RPC provider and streaming logs
/// for a specific subscription made by a process.
async fn handle_subscription_stream(
    our: Arc<String>,
    provider: Provider<Ws>,
    filter: Filter,
    target: Address,
    send_to_loop: MessageSender,
) -> Result<(), EthError> {
    let mut stream = match provider.subscribe_logs(&filter).await {
        Ok(s) => s,
        Err(e) => {
            return Err(EthError::ProviderError(e.to_string()));
        }
    };

    while let Some(event) = stream.next().await {
        send_to_loop
            .send(KernelMessage {
                id: rand::random(),
                source: Address {
                    node: our.to_string(),
                    process: ETH_PROCESS_ID.clone(),
                },
                target: target.clone(),
                rsvp: None,
                message: Message::Request(Request {
                    inherit: false,
                    expects_response: None,
                    body: serde_json::to_vec(&EthSubEvent::Log(event)).unwrap(),
                    metadata: None,
                    capabilities: vec![],
                }),
                lazy_load_blob: None,
            })
            .await
            .unwrap();
    }
    Err(EthError::SubscriptionClosed)
}
