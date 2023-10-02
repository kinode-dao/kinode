use crate::types::*;
use anyhow::Result;
use ethers::core::types::Filter;
use ethers::prelude::Provider;
use ethers::types::{ValueOrArray, U256, U64};
use ethers_providers::{Middleware, StreamExt, Ws};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
enum EthRpcAction {
    SubscribeEvents(EthEventSubscription),
    Unsubscribe(u64),
}

#[derive(Debug, Serialize, Deserialize)]
struct EthEventSubscription {
    addresses: Option<Vec<String>>,
    from_block: Option<u64>,
    to_block: Option<u64>,
    events: Option<Vec<String>>, // aka topic0s
    topic1: Option<U256>,
    topic2: Option<U256>,
    topic3: Option<U256>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthRpcError {
    NoRsvp,
    BadJson,
    NoJson,
    EventSubscriptionFailed,
}
impl EthRpcError {
    pub fn kind(&self) -> &str {
        match *self {
            EthRpcError::NoRsvp { .. } => "NoRsvp",
            EthRpcError::BadJson { .. } => "BapJson",
            EthRpcError::NoJson { .. } => "NoJson",
            EthRpcError::EventSubscriptionFailed { .. } => "EventSubscriptionFailed",
        }
    }
}

pub async fn eth_rpc(
    our: String,
    rpc_url: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    // TODO maybe don't need to do Arc Mutex
    let subscriptions = Arc::new(Mutex::new(HashMap::<
        u64,
        tokio::task::JoinHandle<Result<(), EthRpcError>>,
    >::new()));

    while let Some(message) = recv_in_client.recv().await {
        let our = our.clone();
        let send_to_loop = send_to_loop.clone();
        let print_tx = print_tx.clone();

        let KernelMessage {
            id,
            source,
            ref rsvp,
            message:
                Message::Request(Request {
                    inherit: _,
                    expects_response,
                    ipc: json,
                    metadata: _,
                }),
            ..
        } = message
        else {
            panic!("eth_rpc: bad message");
        };

        let target = if expects_response.is_some() {
            Address {
                node: our.clone(),
                process: source.process.clone(),
            }
        } else {
            let Some(rsvp) = rsvp else {
                send_to_loop
                    .send(make_error_message(
                        our.clone(),
                        id.clone(),
                        source.clone(),
                        EthRpcError::NoRsvp,
                    ))
                    .await
                    .unwrap();
                continue;
            };
            rsvp.clone()
        };

        // let call_data = content.payload.bytes.content.clone().unwrap_or(vec![]);

        let Some(json) = json.clone() else {
            send_to_loop
                .send(make_error_message(
                    our.clone(),
                    id.clone(),
                    source.clone(),
                    EthRpcError::NoJson,
                ))
                .await
                .unwrap();
            continue;
        };

        let Ok(action) = serde_json::from_str::<EthRpcAction>(&json) else {
            send_to_loop
                .send(make_error_message(
                    our.clone(),
                    id.clone(),
                    source.clone(),
                    EthRpcError::BadJson,
                ))
                .await
                .unwrap();
            continue;
        };

        match action {
            EthRpcAction::SubscribeEvents(sub) => {
                let id: u64 = rand::random();
                send_to_loop
                    .send(KernelMessage {
                        id: id.clone(),
                        source: Address {
                            node: our.clone(),
                            process: ProcessId::Name("eth_rpc".into()),
                        },
                        target: target.clone(),
                        rsvp: None,
                        message: Message::Response((
                            Response {
                                ipc: Some(
                                    serde_json::to_string::<Result<u64, EthRpcError>>(&Ok(id))
                                        .unwrap(),
                                ),
                                metadata: None,
                            },
                            None,
                        )),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await
                    .unwrap();

                let mut filter = Filter::new();
                if let Some(addresses) = sub.addresses {
                    filter = filter.address(ValueOrArray::Array(
                        addresses.into_iter().map(|s| s.parse().unwrap()).collect(),
                    ));
                }

                // TODO is there a cleaner way to do all of this?
                if let Some(from_block) = sub.from_block {
                    filter = filter.from_block(from_block);
                }
                if let Some(to_block) = sub.to_block {
                    filter = filter.to_block(to_block);
                }
                if let Some(events) = sub.events {
                    filter = filter.events(&events);
                }
                if let Some(topic1) = sub.topic1 {
                    filter = filter.topic1(topic1);
                }
                if let Some(topic2) = sub.topic2 {
                    filter = filter.topic2(topic2);
                }
                if let Some(topic3) = sub.topic3 {
                    filter = filter.topic3(topic3);
                }

                let rpc_url = rpc_url.clone();

                let handle = tokio::task::spawn(async move {
                    // when connection dies you need to restart at the last block you saw
                    // otherwise you replay events unnecessarily
                    let mut from_block: U64 =
                        filter.clone().get_from_block().unwrap_or(U64::zero());
                    loop {
                        // NOTE give main.rs uses rpc_url and panics if it can't connect, we do
                        // know that this should work in theory...can keep trying to reconnect
                        let Ok(ws_rpc) = Provider::<Ws>::connect(rpc_url.clone()).await else {
                            // TODO grab and print error
                            let _ = print_tx
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!("eth_rpc: connection retrying"),
                                })
                                .await;
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        };

                        match ws_rpc
                            .subscribe_logs(&filter.clone().from_block(from_block))
                            .await
                        {
                            Err(e) => {
                                continue;
                            }
                            Ok(mut stream) => {
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 1,
                                        content: format!("eth_rpc: connection established"),
                                    })
                                    .await;

                                while let Some(event) = stream.next().await {
                                    send_to_loop.send(
                                        KernelMessage {
                                            id: rand::random(),
                                            source: Address {
                                                node: our.clone(),
                                                process: ProcessId::Name("eth_rpc".into()),
                                            },
                                            target: target.clone(),
                                            rsvp: None,
                                            message: Message::Request(Request {
                                                inherit: false, // TODO what
                                                expects_response: None,
                                                ipc: Some(json!({
                                                    "EventSubscription": serde_json::to_value(event.clone()).unwrap()
                                                }).to_string()),
                                                metadata: None,
                                            }),
                                            payload: None,
                                            signed_capabilities: None,
                                        }
                                    ).await.unwrap();
                                    from_block = event.block_number.unwrap_or(from_block);
                                }
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 0,
                                        content: format!(
                                            "eth_rpc: subscription connection lost, reconnecting"
                                        ),
                                    })
                                    .await;
                            }
                        };
                    }
                });
                subscriptions.lock().await.insert(id, handle);
            }
            EthRpcAction::Unsubscribe(sub_id) => {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("eth_rpc: unsubscribing from {}", sub_id),
                    })
                    .await;

                if let Some(handle) = subscriptions.lock().await.remove(&sub_id) {
                    handle.abort();
                } else {
                    let _ = print_tx
                        .send(Printout {
                            verbosity: 0,
                            content: format!("eth_rpc: no task found with id {}", sub_id),
                        })
                        .await;
                }
            }
        }
    }

    Ok(())
}

//
//  helpers
//

fn make_error_message(our: String, id: u64, source: Address, error: EthRpcError) -> KernelMessage {
    KernelMessage {
        id,
        source: Address {
            node: our.clone(),
            process: ProcessId::Name("eth_rpc".into()),
        },
        target: source,
        rsvp: None,
        message: Message::Response((
            Response {
                ipc: Some(serde_json::to_string::<Result<u64, EthRpcError>>(&Err(error)).unwrap()),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
