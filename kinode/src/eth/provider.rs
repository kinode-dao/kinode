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
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

/// mapping of chain id to ordered(TODO) list of providers
type Providers = Arc<DashMap<u64, ActiveProviders>>;

struct ActiveProviders {
    pub urls: Vec<UrlProvider>,
    pub nodes: Vec<NodeProvider>,
}

struct UrlProvider {
    pub trusted: bool,
    pub url: String,
    pub pubsub: Option<Provider<PubSubFrontend>>,
}

struct NodeProvider {
    pub trusted: bool,
    pub name: String,
}

/// existing subscriptions held by local processes
type ActiveSubscriptions = Arc<DashMap<ProcessId, HashMap<u64, ActiveSub>>>;

enum ActiveSub {
    Local(JoinHandle<Result<(), EthError>>),
    Remote(String), // name of node providing this subscription for us
}

impl ActiveProviders {
    fn add_provider_config(&mut self, new: ProviderConfig) {
        match new.provider {
            NodeOrRpcUrl::Node(update) => {
                self.nodes.push(NodeProvider {
                    trusted: new.trusted,
                    name: update.name,
                });
            }
            NodeOrRpcUrl::RpcUrl(url) => {
                self.urls.push(UrlProvider {
                    trusted: new.trusted,
                    url,
                    pubsub: None,
                });
            }
        }
    }

    fn remove_provider(&mut self, remove: &str) {
        self.urls.retain(|x| x.url != remove);
        self.nodes.retain(|x| x.name != remove);
    }
}

async fn activate_url_provider(provider: &mut UrlProvider) -> Result<()> {
    match Url::parse(&provider.url)?.scheme() {
        "ws" | "wss" => {
            let connector = WsConnect {
                url: provider.url.to_string(),
                auth: None,
            };
            let client = ClientBuilder::default().ws(connector).await?;
            provider.pubsub = Some(Provider::new_with_client(client));
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "Only `ws://` or `wss://` providers are supported."
        )),
    }
}

/// The ETH provider runtime process is responsible for connecting to one or more ETH RPC providers
/// and using them to service indexing requests from other apps.
pub async fn provider(
    our: String,
    configs: SavedConfigs,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    caps_oracle: CapMessageSender,
    print_tx: PrintSender,
) -> Result<()> {
    let our = Arc::new(our);

    let mut access_settings = AccessSettings {
        public: false,
        allow: HashSet::new(),
        deny: HashSet::new(),
    };

    // convert saved configs into data structure that we will use to route queries
    let mut providers: Providers = Arc::new(DashMap::new());
    for entry in configs {
        let mut ap = providers.entry(entry.chain_id).or_insert(ActiveProviders {
            urls: vec![],
            nodes: vec![],
        });
        ap.add_provider_config(entry);
    }

    // handles of longrunning subscriptions.
    let mut active_subscriptions: ActiveSubscriptions = Arc::new(DashMap::new());

    while let Some(km) = recv_in_client.recv().await {
        let km_id = km.id;
        let response_target = km.rsvp.as_ref().unwrap_or(&km.source).clone();
        if let Err(e) = handle_message(
            &our,
            &mut access_settings,
            km,
            &send_to_loop,
            &caps_oracle,
            &mut providers,
            &mut active_subscriptions,
        )
        .await
        {
            send_to_loop
                .send(make_error_message(&our, km_id, response_target, e))
                .await
                .expect("eth: kernel sender died!");
        };
    }
    Err(anyhow::anyhow!("eth: fatal: message receiver closed!"))
}

/// handle incoming requests, namely [`EthAction`] and [`EthConfigAction`].
/// also handle responses that are passthroughs from remote provider nodes.
async fn handle_message(
    our: &str,
    access_settings: &mut AccessSettings,
    km: KernelMessage,
    send_to_loop: &MessageSender,
    caps_oracle: &CapMessageSender,
    providers: &mut Providers,
    active_subscriptions: &mut ActiveSubscriptions,
) -> Result<(), EthError> {
    match &km.message {
        Message::Response(_) => handle_passthrough_response(our, send_to_loop, km).await,
        Message::Request(req) => {
            if let Ok(eth_action) = serde_json::from_slice(&req.body) {
                // these can be from remote or local processes
                return handle_eth_action(
                    our,
                    access_settings,
                    km,
                    eth_action,
                    providers,
                    active_subscriptions,
                )
                .await;
            }
            if let Ok(eth_config_action) = serde_json::from_slice(&req.body) {
                // only local node
                return handle_eth_config_action(
                    our,
                    access_settings,
                    caps_oracle,
                    km,
                    eth_config_action,
                    providers,
                )
                .await;
            }
            Err(EthError::PermissionDenied)
        }
    }
}

async fn handle_passthrough_response(
    our: &str,
    send_to_loop: &MessageSender,
    km: KernelMessage,
) -> Result<(), EthError> {
    send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our.to_string(),
                process: ETH_PROCESS_ID.clone(),
            },
            target: km.rsvp.unwrap_or(km.source),
            rsvp: None,
            message: km.message,
            lazy_load_blob: None,
        })
        .await
        .expect("eth: kernel sender died!");
    Ok(())
}

async fn handle_eth_action(
    our: &str,
    access_settings: &mut AccessSettings,
    km: KernelMessage,
    eth_action: EthAction,
    providers: &mut Providers,
    active_subscriptions: &mut ActiveSubscriptions,
) -> Result<(), EthError> {
    // check our access settings if the request is from a remote node
    if km.source.node != our {
        if !access_settings.deny.contains(&km.source.node) {
            if !access_settings.public {
                if !access_settings.allow.contains(&km.source.node) {
                    return Err(EthError::PermissionDenied);
                }
            }
        } else {
            return Err(EthError::PermissionDenied);
        }
    }

    // for each incoming action, we need to assign a provider from our map
    // based on the chain id. once we assign a provider, we can use it for
    // this request. if the provider is not usable, cycle through options
    // before returning an error.
    match eth_action {
        EthAction::SubscribeLogs {
            sub_id,
            chain_id,
            kind,
            params,
        } => {
            todo!()
        }
        EthAction::UnsubscribeLogs(sub_id) => {
            active_subscriptions
                .entry(km.source.process)
                .and_modify(|sub_map| {
                    sub_map.remove(&sub_id);
                });
            Ok(())
        }
        EthAction::Request {
            chain_id,
            method,
            params,
        } => {
            todo!()
        }
    }
}

async fn handle_eth_config_action(
    our: &str,
    access_settings: &mut AccessSettings,
    caps_oracle: &CapMessageSender,
    km: KernelMessage,
    eth_config_action: EthConfigAction,
    providers: &mut Providers,
) -> Result<(), EthError> {
    if km.source.node != our {
        return Err(EthError::PermissionDenied);
    }
    // check capabilities to ensure the sender is allowed to make this request
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    caps_oracle
        .send(CapMessage::Has {
            on: km.source.process.clone(),
            cap: Capability {
                issuer: Address {
                    node: our.to_string(),
                    process: ETH_PROCESS_ID.clone(),
                },
                params: serde_json::to_string(&serde_json::json!({
                    "root": true,
                }))
                .unwrap(),
            },
            responder: send_cap_bool,
        })
        .await
        .expect("eth: capability oracle died!");
    if !recv_cap_bool.await.unwrap_or(false) {
        return Err(EthError::PermissionDenied);
    }

    // modify our providers and access settings based on config action
    todo!()
}

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
    loop {
        match rx.recv().await {
            Err(e) => {
                return Err(EthError::SubscriptionClosed(sub_id));
            }
            Ok(value) => {
                let result: SubscriptionResult =
                    serde_json::from_str(value.get()).map_err(|_| {
                        EthError::RpcError(
                            "eth: failed to deserialize subscription result".to_string(),
                        )
                    })?;
                send_to_loop
                    .send(KernelMessage {
                        id: rand::random(),
                        source: Address {
                            node: our.clone(),
                            process: ETH_PROCESS_ID.clone(),
                        },
                        target: target.clone(),
                        rsvp: rsvp.clone(),
                        message: Message::Request(Request {
                            inherit: false,
                            expects_response: None,
                            body: serde_json::to_vec(&EthSubResult::Ok(EthSub {
                                id: sub_id,
                                result,
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
    }
}

fn make_error_message(our: &str, id: u64, target: Address, error: EthError) -> KernelMessage {
    KernelMessage {
        id,
        source: Address {
            node: our.to_string(),
            process: ETH_PROCESS_ID.clone(),
        },
        target,
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
