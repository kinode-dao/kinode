use alloy_providers::provider::Provider;
use alloy_pubsub::{PubSubFrontend, RawSubscription};
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::pubsub::SubscriptionResult;
use alloy_transport_ws::WsConnect;
use anyhow::Result;
use dashmap::DashMap;
use lib::types::core::*;
use lib::types::eth::*;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

/// The ETH provider runtime process is responsible for connecting to one or more ETH RPC providers
/// and using them to service indexing requests from other apps. This could also be done by a Wasm
/// app, but in the future, this process will hopefully expand in scope to perform more complex
/// indexing and ETH node responsibilities.
pub async fn provider(
    our: String,
    configs: SavedConfigs,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    print_tx: PrintSender,
) -> Result<()> {
    let our = Arc::new(our);

    // // Initialize the provider conditionally based on rpc_url
    // // Todo: make provider<T> support multiple transports, one direct and another passthrough.
    // let provider_config = match provider_node {
    //     ProviderInput::Ws(rpc_url) => {
    //         // Validate and parse the WebSocket URL
    //         match Url::parse(&rpc_url)?.scheme() {
    //             "ws" | "wss" => {
    //                 let connector = WsConnect {
    //                     url: rpc_url,
    //                     auth: None,
    //                 };
    //                 let client = ClientBuilder::default().ws(connector).await?;
    //                 ProviderConfig::Provider(Provider::new_with_client(client))
    //             }
    //             _ => {
    //                 return Err(anyhow::anyhow!(
    //                     "Only `ws://` or `wss://` URLs are supported."
    //                 ))
    //             }
    //         }
    //     }
    //     ProviderInput::Node(node_id) => {
    //         // Directly use the node ID
    //         ProviderConfig::Node(node_id)
    //     }
    // };

    // let provider_config = Arc::new(provider_config);

    // // handles of longrunning subscriptions.
    // let connections: DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>> = DashMap::new();
    // let connections = Arc::new(connections);

    // while let Some(km) = recv_in_client.recv().await {
    //     // clone Arcs
    //     let our = our.clone();
    //     let send_to_loop = send_to_loop.clone();
    //     let provider_config = provider_config.clone();
    //     let connections = connections.clone();
    //     tokio::spawn(async move {
    //         if let Err(e) = handle_message(
    //             &our,
    //             &km,
    //             &send_to_loop,
    //             provider_config,
    //             connections,
    //             public,
    //         )
    //         .await
    //         {
    //             let _ = send_to_loop
    //                 .send(make_error_message(our.to_string(), km, e))
    //                 .await;
    //         };
    //     });
    // }
    Err(anyhow::anyhow!("eth: fatal: message receiver closed!"))
}

/// Handle an incoming message.
// async fn handle_message(
//     our: &str,
//     km: &KernelMessage,
//     send_to_loop: &MessageSender,
//     provider_config: Arc<ProviderConfig>,
//     connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
//     public: bool,
// ) -> Result<(), EthError> {
//     match &km.message {
//         Message::Request(req) => {
//             match &*provider_config {
//                 ProviderConfig::Node(node) => {
//                     if km.source.node == our {
//                         // we have no provider, let's send this request to someone who has one.
//                         let request = KernelMessage {
//                             id: km.id,
//                             source: Address {
//                                 node: our.to_string(),
//                                 process: ETH_PROCESS_ID.clone(),
//                             },
//                             target: Address {
//                                 node: "jugodenaranja.os".to_string(),
//                                 process: ETH_PROCESS_ID.clone(),
//                             },
//                             rsvp: Some(km.source.clone()),
//                             message: Message::Request(req.clone()),
//                             lazy_load_blob: None,
//                         };

//                         let _ = send_to_loop.send(request).await;
//                     } else {
//                         // either someone asking us for rpc, or we are passing through a sub event.
//                         handle_remote_request(our, km, send_to_loop, None, connections, public)
//                             .await?
//                     }
//                 }
//                 ProviderConfig::Provider(provider) => {
//                     if km.source.node == our {
//                         handle_local_request(our, km, send_to_loop, &provider, connections, public)
//                             .await?
//                     } else {
//                         handle_remote_request(
//                             our,
//                             km,
//                             send_to_loop,
//                             Some(provider),
//                             connections,
//                             public,
//                         )
//                         .await?
//                     }
//                 }
//             }
//         }
//         Message::Response(_) => {
//             // handle passthrough responses, send to rsvp.
//             if km.source.process == "eth:distro:sys" {
//                 if let Some(rsvp) = &km.rsvp {
//                     let _ = send_to_loop
//                         .send(KernelMessage {
//                             id: rand::random(),
//                             source: Address {
//                                 node: our.to_string(),
//                                 process: ETH_PROCESS_ID.clone(),
//                             },
//                             target: rsvp.clone(),
//                             rsvp: None,
//                             message: km.message.clone(),
//                             lazy_load_blob: None,
//                         })
//                         .await;
//                 }
//             }
//         }
//     }
//     Ok(())
// }

/// Handle a local request.
// async fn handle_local_request(
//     our: &str,
//     km: &KernelMessage,
//     send_to_loop: &MessageSender,
//     provider: &Provider<PubSubFrontend>,
//     connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
//     public: bool,
// ) -> Result<(), EthError> {
//     let Message::Request(req) = &km.message else {
//         return Err(EthError::InvalidMethod(
//             "eth: only accepts requests".to_string(),
//         ));
//     };
//     let action = serde_json::from_slice::<EthAction>(&req.body).map_err(|e| {
//         EthError::InvalidMethod(format!("eth: failed to deserialize request: {:?}", e))
//     })?;

//     // we might want some of these in payloads.. sub items?
//     let return_body: EthResponse = match action {
//         EthAction::SubscribeLogs {
//             sub_id,
//             kind,
//             params,
//         } => {
//             let sub_id = (km.target.process.clone(), sub_id);

//             let kind = serde_json::to_value(&kind).unwrap();
//             let params = serde_json::to_value(&params).unwrap();

//             let id = provider
//                 .inner()
//                 .prepare("eth_subscribe", [kind, params])
//                 .await
//                 .map_err(|e| EthError::TransportError(e.to_string()))?;

//             let rx = provider.inner().get_raw_subscription(id).await;
//             let handle = tokio::spawn(handle_subscription_stream(
//                 our.to_string(),
//                 sub_id.1.clone(),
//                 rx,
//                 km.source.clone(),
//                 km.rsvp.clone(),
//                 send_to_loop.clone(),
//             ));

//             connections.insert(sub_id, handle);
//             EthResponse::Ok
//         }
//         EthAction::UnsubscribeLogs(sub_id) => {
//             let sub_id = (km.target.process.clone(), sub_id);
//             let handle = connections
//                 .remove(&sub_id)
//                 .ok_or(EthError::SubscriptionNotFound)?;

//             handle.1.abort();
//             EthResponse::Ok
//         }
//         EthAction::Request { method, params } => {
//             let method = to_static_str(&method).ok_or(EthError::InvalidMethod(method))?;

//             let response: serde_json::Value = provider
//                 .inner()
//                 .prepare(method, params)
//                 .await
//                 .map_err(|e| EthError::TransportError(e.to_string()))?;
//             EthResponse::Response { value: response }
//         }
//     };
//     if let Some(_) = req.expects_response {
//         let _ = send_to_loop
//             .send(KernelMessage {
//                 id: km.id,
//                 source: Address {
//                     node: our.to_string(),
//                     process: ETH_PROCESS_ID.clone(),
//                 },
//                 target: km.source.clone(),
//                 rsvp: km.rsvp.clone(),
//                 message: Message::Response((
//                     Response {
//                         inherit: false,
//                         body: serde_json::to_vec(&return_body).unwrap(),
//                         metadata: req.metadata.clone(),
//                         capabilities: vec![],
//                     },
//                     None,
//                 )),
//                 lazy_load_blob: None,
//             })
//             .await;
//     }

//     Ok(())
// }

/// here we are either processing another nodes request.
/// or we are passing through an ethSub Request..
// async fn handle_remote_request(
//     our: &str,
//     km: &KernelMessage,
//     send_to_loop: &MessageSender,
//     provider: Option<&Provider<PubSubFrontend>>,
//     connections: Arc<DashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>>,
//     public: bool,
// ) -> Result<(), EthError> {
//     let Message::Request(req) = &km.message else {
//         return Err(EthError::InvalidMethod(
//             "eth: only accepts requests".to_string(),
//         ));
//     };

//     if let Some(provider) = provider {
//         // we need some sort of agreement perhaps on rpc providing.
//         // even with an agreement, fake ethsubevents could be sent to us.
//         // light clients could verify blocks perhaps...
//         if !public {
//             return Err(EthError::PermissionDenied("not on the list.".to_string()));
//         }

//         let action = serde_json::from_slice::<EthAction>(&req.body).map_err(|e| {
//             EthError::InvalidMethod(format!("eth: failed to deserialize request: {:?}", e))
//         })?;

//         let return_body: EthResponse = match action {
//             EthAction::SubscribeLogs {
//                 sub_id,
//                 kind,
//                 params,
//             } => {
//                 let sub_id = (km.target.process.clone(), sub_id);

//                 let kind = serde_json::to_value(&kind).unwrap();
//                 let params = serde_json::to_value(&params).unwrap();

//                 let id = provider
//                     .inner()
//                     .prepare("eth_subscribe", [kind, params])
//                     .await
//                     .map_err(|e| EthError::TransportError(e.to_string()))?;

//                 let rx = provider.inner().get_raw_subscription(id).await;
//                 let handle = tokio::spawn(handle_subscription_stream(
//                     our.to_string(),
//                     sub_id.1.clone(),
//                     rx,
//                     km.target.clone(),
//                     km.rsvp.clone(),
//                     send_to_loop.clone(),
//                 ));

//                 connections.insert(sub_id, handle);
//                 EthResponse::Ok
//             }
//             EthAction::UnsubscribeLogs(sub_id) => {
//                 let sub_id = (km.target.process.clone(), sub_id);
//                 let handle = connections
//                     .remove(&sub_id)
//                     .ok_or(EthError::SubscriptionNotFound)?;

//                 handle.1.abort();
//                 EthResponse::Ok
//             }
//             EthAction::Request { method, params } => {
//                 let method = to_static_str(&method).ok_or(EthError::InvalidMethod(method))?;

//                 let response: serde_json::Value = provider
//                     .inner()
//                     .prepare(method, params)
//                     .await
//                     .map_err(|e| EthError::TransportError(e.to_string()))?;

//                 EthResponse::Response { value: response }
//             }
//         };

//         let response = KernelMessage {
//             id: km.id,
//             source: Address {
//                 node: our.to_string(),
//                 process: ETH_PROCESS_ID.clone(),
//             },
//             target: km.source.clone(),
//             rsvp: km.rsvp.clone(),
//             message: Message::Response((
//                 Response {
//                     inherit: false,
//                     body: serde_json::to_vec(&return_body).unwrap(),
//                     metadata: req.metadata.clone(),
//                     capabilities: vec![],
//                 },
//                 None,
//             )),
//             lazy_load_blob: None,
//         };

//         let _ = send_to_loop.send(response).await;
//     } else {
//         // We do not have a provider, this is a reply for a request made by us.
//         if let Ok(eth_sub) = serde_json::from_slice::<EthSub>(&req.body) {
//             // forward...
//             if let Some(target) = km.rsvp.clone() {
//                 let _ = send_to_loop
//                     .send(KernelMessage {
//                         id: rand::random(),
//                         source: Address {
//                             node: our.to_string(),
//                             process: ETH_PROCESS_ID.clone(),
//                         },
//                         target: target,
//                         rsvp: None,
//                         message: Message::Request(req.clone()),
//                         lazy_load_blob: None,
//                     })
//                     .await;
//             }
//         }
//     }
//     Ok(())
// }

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
            let error = Err(EthError::SubscriptionClosed(sub_id))?;
            let _ = send_to_loop
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
                        body: serde_json::to_vec(&EthSubResult::Err(EthSubError {
                            id: sub_id,
                            error: e.to_string(),
                        }))
                        .unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    }),
                    lazy_load_blob: None,
                })
                .await
                .unwrap();
        }
        Ok(value) => {
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
                        body: serde_json::to_vec(&EthSubResult::Ok(EthSub {
                            id: sub_id,
                            result: event,
                        }))
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
