use crate::types::*;
use anyhow::Result;
use ethers::core::types::Filter;
use ethers::prelude::Provider;
use ethers::types::{ValueOrArray, U256, U64};
use ethers_providers::{Middleware, StreamExt, Ws};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

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
    pub fn _kind(&self) -> &str {
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
    let mut subscriptions = HashMap::<u64, tokio::task::JoinHandle<Result<(), EthRpcError>>>::new();

    while let Some(message) = recv_in_client.recv().await {
        let our = our.clone();
        let send_to_loop = send_to_loop.clone();
        let print_tx = print_tx.clone();

        let KernelMessage {
            ref source,
            ref rsvp,
            message:
                Message::Request(Request {
                    expects_response,
                    ipc: ref json_bytes,
                    ..
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
                        &message,
                        EthRpcError::NoRsvp,
                    ))
                    .await
                    .unwrap();
                continue;
            };
            rsvp.clone()
        };

        // let call_data = content.payload.bytes.content.clone().unwrap_or(vec![]);

        let Ok(action) = serde_json::from_slice::<EthRpcAction>(json_bytes) else {
            send_to_loop
                .send(make_error_message(
                    our.clone(),
                    &message,
                    EthRpcError::BadJson,
                ))
                .await
                .unwrap();
            continue;
        };

        match action {
            EthRpcAction::SubscribeEvents(sub) => {
                send_to_loop
                    .send(KernelMessage {
                        id: message.id,
                        source: Address {
                            node: our.clone(),
                            process: ETH_RPC_PROCESS_ID.clone(),
                        },
                        target: match &message.rsvp {
                            None => message.source.clone(),
                            Some(rsvp) => rsvp.clone(),
                        },
                        rsvp: None,
                        message: Message::Response((
                            Response {
                                inherit: false,
                                ipc: serde_json::to_vec::<Result<u64, EthRpcError>>(
                                    &Ok(message.id),
                                )
                                .unwrap(),
                                metadata: None,
                                capabilities: vec![],
                            },
                            None,
                        )),
                        payload: None,
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
                                    verbosity: 0,
                                    content: "eth_rpc: connection failed, retrying in 5s"
                                        .to_string(),
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
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 0,
                                        content: format!("eth_rpc: subscription error: {:?}", e),
                                    })
                                    .await;
                                continue;
                            }
                            Ok(mut stream) => {
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 0,
                                        content: "eth_rpc: connection established".to_string(),
                                    })
                                    .await;

                                while let Some(event) = stream.next().await {
                                    send_to_loop.send(
                                        KernelMessage {
                                            id: rand::random(),
                                            source: Address {
                                                node: our.clone(),
                                                process: ETH_RPC_PROCESS_ID.clone(),
                                            },
                                            target: target.clone(),
                                            rsvp: None,
                                            message: Message::Request(Request {
                                                inherit: false,
                                                expects_response: None,
                                                ipc: json!({
                                                    "EventSubscription": serde_json::to_value(event.clone()).unwrap()
                                                }).to_string().into_bytes(),
                                                metadata: None,
                                                capabilities: vec![],
                                            }),
                                            payload: None,

                                        }
                                    ).await.unwrap();
                                    from_block = event.block_number.unwrap_or(from_block);
                                }
                                let _ = print_tx
                                    .send(Printout {
                                        verbosity: 0,
                                        content:
                                            "eth_rpc: subscription connection lost, reconnecting"
                                                .to_string(),
                                    })
                                    .await;
                            }
                        };
                    }
                });
                subscriptions.insert(message.id, handle);
            }
            EthRpcAction::Unsubscribe(sub_id) => {
                let _ = print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("eth_rpc: unsubscribing from {}", sub_id),
                    })
                    .await;

                if let Some(handle) = subscriptions.remove(&sub_id) {
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

fn make_error_message(our_name: String, km: &KernelMessage, error: EthRpcError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: ETH_RPC_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec::<Result<u64, EthRpcError>>(&Err(error)).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        payload: None,
    }
}
