use alloy_providers::provider::Provider;
use alloy_pubsub::{PubSubFrontend, RawSubscription};
use alloy_rpc_client::ClientBuilder;
use alloy_rpc_types::pubsub::SubscriptionResult;
use alloy_transport_ws::WsConnect;
use anyhow::Result;
use dashmap::DashMap;
use futures::Future;
use lib::types::core::*;
use lib::types::eth::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

/// mapping of chain id to ordered lists of providers
type Providers = Arc<DashMap<u64, ActiveProviders>>;

#[derive(Debug)]
struct ActiveProviders {
    pub urls: Vec<UrlProvider>,
    pub nodes: Vec<NodeProvider>,
}

#[derive(Debug)]
struct UrlProvider {
    pub trusted: bool,
    pub url: String,
    pub pubsub: Option<Provider<PubSubFrontend>>,
}

#[derive(Debug)]
struct NodeProvider {
    pub trusted: bool,
    pub name: String,
}

/// existing subscriptions held by local processes
type ActiveSubscriptions = Arc<DashMap<ProcessId, HashMap<u64, ActiveSub>>>;

#[derive(Debug)]
enum ActiveSub {
    Local(JoinHandle<()>),
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
    println!("provider: activate_url_provider\r");
    match Url::parse(&provider.url)?.scheme() {
        "ws" | "wss" => {
            let connector = WsConnect {
                url: provider.url.to_string(),
                auth: None,
            };
            println!("here1\r");
            let client = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                ClientBuilder::default().ws(connector),
            )
            .await??;
            println!("here2\r");
            provider.pubsub = Some(Provider::new_with_client(client));
            println!("here3\r");
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
    println!("provider: on\r");
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

    println!("providers: {providers:?}\r");

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
            let _ = send_to_loop
                .send(make_error_message(&our, km_id, response_target, e))
                .await;
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
    println!("provider: handle_message\r");
    match &km.message {
        Message::Response(_) => handle_passthrough_response(our, send_to_loop, km).await,
        Message::Request(req) => {
            let timeout = *req.expects_response.as_ref().unwrap_or(&60); // TODO make this a config
            if let Ok(eth_action) = serde_json::from_slice(&req.body) {
                // these can be from remote or local processes
                return handle_eth_action(
                    our,
                    access_settings,
                    send_to_loop,
                    km,
                    timeout,
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
    println!("provider: handle_passthrough_response\r");
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
    send_to_loop: &MessageSender,
    km: KernelMessage,
    timeout: u64,
    eth_action: EthAction,
    providers: &mut Providers,
    active_subscriptions: &mut ActiveSubscriptions,
) -> Result<(), EthError> {
    println!("provider: handle_eth_action: {eth_action:?}\r");
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
        EthAction::SubscribeLogs { sub_id, .. } => {
            // let new_sub = ActiveSub::Local(tokio::spawn(create_new_subscription(
            //     our.to_string(),
            //     km.id,
            //     km.source.clone(),
            //     km.rsvp,
            //     send_to_loop.clone(),
            //     eth_action,
            //     providers.clone(),
            //     active_subscriptions.clone(),
            // )));
            // let mut subs = active_subscriptions
            //     .entry(km.source.process)
            //     .or_insert(HashMap::new());
            // subs.insert(sub_id, new_sub);
            create_new_subscription(
                our.to_string(),
                km.id,
                km.source.clone(),
                km.rsvp,
                send_to_loop.clone(),
                eth_action,
                providers.clone(),
                active_subscriptions.clone(),
            )
            .await
        }
        EthAction::UnsubscribeLogs(sub_id) => {
            active_subscriptions
                .entry(km.source.process)
                .and_modify(|sub_map| {
                    if let Some(sub) = sub_map.get_mut(&sub_id) {
                        match sub {
                            ActiveSub::Local(handle) => {
                                handle.abort();
                            }
                            ActiveSub::Remote(node) => {
                                // TODO send to them asking to abort
                            }
                        }
                    }
                });
        }
        EthAction::Request { .. } => {
            fulfill_request(
                our.to_string(),
                km.id,
                km.source.clone(),
                km.rsvp,
                timeout,
                send_to_loop.clone(),
                eth_action,
                providers.clone(),
            )
            .await;
        }
    }
    Ok(())
}

/// cleans itself up when the subscription is closed or fails.
async fn create_new_subscription(
    our: String,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    send_to_loop: MessageSender,
    eth_action: EthAction,
    providers: Providers,
    active_subscriptions: ActiveSubscriptions,
) {
    println!("provider: create_new_subscription\r");
    match build_subscription(
        our.clone(),
        km_id,
        target.clone(),
        rsvp.clone(),
        send_to_loop.clone(),
        &eth_action,
        providers,
    )
    .await
    {
        Ok(future) => {
            // send a response to the target that the subscription was successful
            send_to_loop
                .send(KernelMessage {
                    id: km_id,
                    source: Address {
                        node: our.to_string(),
                        process: ETH_PROCESS_ID.clone(),
                    },
                    target: target.clone(),
                    rsvp: rsvp.clone(),
                    message: Message::Response((
                        Response {
                            inherit: false,
                            body: serde_json::to_vec(&EthResponse::Ok).unwrap(),
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )),
                    lazy_load_blob: None,
                })
                .await
                .expect("eth: sender died!");
            // await the subscription error and kill it if so
            if let Err(e) = future.await {
                let _ = send_to_loop
                    .send(make_error_message(&our, km_id, target.clone(), e))
                    .await;
            }
        }
        Err(e) => {
            let _ = send_to_loop
                .send(make_error_message(&our, km_id, target.clone(), e))
                .await;
        }
    }
    active_subscriptions
        .entry(target.process)
        .and_modify(|sub_map| {
            sub_map.remove(&km_id);
        });
}

async fn build_subscription(
    our: String,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    send_to_loop: MessageSender,
    eth_action: &EthAction,
    providers: Providers,
) -> Result<impl Future<Output = Result<(), EthError>>, EthError> {
    println!("provider: build_subscription\r");
    let EthAction::SubscribeLogs {
        sub_id,
        chain_id,
        kind,
        params,
    } = eth_action
    else {
        return Err(EthError::InvalidMethod(
            "eth: only accepts subscribe logs requests".to_string(),
        ));
    };
    let Some(mut aps) = providers.get_mut(&chain_id) else {
        return Err(EthError::NoRpcForChain);
    };
    // first, try any url providers we have for this chain,
    // then if we have none or they all fail, go to node providers.
    // finally, if no provider works, return an error.
    for url_provider in &mut aps.urls {
        let pubsub = match &url_provider.pubsub {
            Some(pubsub) => pubsub,
            None => {
                if let Ok(()) = activate_url_provider(url_provider).await {
                    url_provider.pubsub.as_ref().unwrap()
                } else {
                    continue;
                }
            }
        };
        let kind = serde_json::to_value(&kind).unwrap();
        let params = serde_json::to_value(&params).unwrap();
        if let Ok(id) = pubsub
            .inner()
            .prepare("eth_subscribe", [kind, params])
            .await
        {
            let rx = pubsub.inner().get_raw_subscription(id).await;
            return Ok(maintain_subscription(
                our,
                *sub_id,
                rx,
                target,
                rsvp,
                send_to_loop,
            ));
        }
        // this provider failed and needs to be reset
        url_provider.pubsub = None;
    }
    for node_provider in &aps.nodes {
        // todo
    }
    return Err(EthError::NoRpcForChain);
}

async fn maintain_subscription(
    our: String,
    sub_id: u64,
    mut rx: RawSubscription,
    target: Address,
    rsvp: Option<Address>,
    send_to_loop: MessageSender,
) -> Result<(), EthError> {
    println!("provider: maintain_subscription\r");
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
                            node: our.to_string(),
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
                    .map_err(|_| EthError::RpcError("eth: sender died".to_string()))?;
            }
        }
    }
}

async fn fulfill_request(
    our: String,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    timeout: u64,
    send_to_loop: MessageSender,
    eth_action: EthAction,
    providers: Providers,
) {
    println!("provider: fulfill_request\r");
    let EthAction::Request {
        chain_id,
        method,
        params,
    } = eth_action
    else {
        return;
    };
    let Some(method) = to_static_str(&method) else {
        let _ = send_to_loop
            .send(make_error_message(
                &our,
                km_id,
                target,
                EthError::InvalidMethod(method),
            ))
            .await;
        return;
    };
    let Some(mut aps) = providers.get_mut(&chain_id) else {
        let _ = send_to_loop
            .send(make_error_message(
                &our,
                km_id,
                target,
                EthError::NoRpcForChain,
            ))
            .await;
        return;
    };
    // first, try any url providers we have for this chain,
    // then if we have none or they all fail, go to node providers.
    // finally, if no provider works, return an error.
    for url_provider in &mut aps.urls {
        let pubsub = match &url_provider.pubsub {
            Some(pubsub) => pubsub,
            None => {
                if let Ok(()) = activate_url_provider(url_provider).await {
                    url_provider.pubsub.as_ref().unwrap()
                } else {
                    continue;
                }
            }
        };
        println!("here5\r");
        let connector = WsConnect {
            url: url_provider.url.to_string(),
            auth: None,
        };
        let client = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            ClientBuilder::default().ws(connector),
        )
        .await.unwrap().unwrap();
        println!("here6\r");
        let provider = Provider::new_with_client(client);
        println!("method: {method:?}\r");
        println!("params: {params:?}\r");
        let response = provider.inner().prepare(method, params.clone()).await;
        println!("res: {response:?}\r");
        // let Ok(response) = tokio::time::timeout(
        //     std::time::Duration::from_secs(timeout),
        //     pubsub.inner().prepare(method, params.clone()),
        // )
        // .await
        // else {
        //     println!("what the FUCK\r");
        //     // this provider failed and needs to be reset
        //     url_provider.pubsub = None;
        //     continue;
        // };
        println!("here6\r");
        if let Ok(value) = response {
            send_to_loop
                .send(KernelMessage {
                    id: km_id,
                    source: Address {
                        node: our.to_string(),
                        process: ETH_PROCESS_ID.clone(),
                    },
                    target,
                    rsvp,
                    message: Message::Response((
                        Response {
                            inherit: false,
                            body: serde_json::to_vec(&EthResponse::Response { value }).unwrap(),
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )),
                    lazy_load_blob: None,
                })
                .await
                .expect("eth: sender died!");
            return;
        }
        // this provider failed and needs to be reset
        url_provider.pubsub = None;
    }
    for node_provider in &aps.nodes {
        // todo
    }
    let _ = send_to_loop
        .send(make_error_message(
            &our,
            km_id,
            target,
            EthError::NoRpcForChain,
        ))
        .await;
}

async fn handle_eth_config_action(
    our: &str,
    access_settings: &mut AccessSettings,
    caps_oracle: &CapMessageSender,
    km: KernelMessage,
    eth_config_action: EthConfigAction,
    providers: &mut Providers,
) -> Result<(), EthError> {
    println!("provider: handle_eth_config_action\r");
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

fn make_error_message(our: &str, km_id: u64, target: Address, error: EthError) -> KernelMessage {
    println!("provider: make_error_message\r");
    KernelMessage {
        id: km_id,
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
