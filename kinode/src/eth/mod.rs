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
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

/// meta-type for all incoming requests we need to handle
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum IncomingReq {
    EthAction(EthAction),
    EthConfigAction(EthConfigAction),
    EthSubResult(EthSubResult),
}

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
    /// semi-temporary flag to mark if this provider is currently usable
    /// future updates will make this more dynamic
    pub usable: bool,
    pub name: String,
}

/// existing subscriptions held by local OR remote processes
type ActiveSubscriptions = Arc<DashMap<Address, HashMap<u64, ActiveSub>>>;

type ResponseChannels = Arc<DashMap<u64, ProcessMessageSender>>;

#[derive(Debug)]
enum ActiveSub {
    Local(JoinHandle<()>),
    Remote(String), // name of node providing this subscription for us
}

impl ActiveProviders {
    fn add_provider_config(&mut self, new: ProviderConfig) {
        match new.provider {
            NodeOrRpcUrl::Node {
                kns_update,
                use_as_provider,
            } => {
                self.nodes.push(NodeProvider {
                    trusted: new.trusted,
                    usable: use_as_provider,
                    name: kns_update.name,
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
            let client = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                ClientBuilder::default().ws(connector),
            )
            .await??;
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
    mut net_error_recv: NetworkErrorReceiver,
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

    // channels to pass incoming responses to outstanding requests
    // keyed by KM ID
    let mut response_channels: Arc<DashMap<u64, ProcessMessageSender>> = Arc::new(DashMap::new());

    loop {
        tokio::select! {
            Some(wrapped_error) = net_error_recv.recv() => {
                let _ = print_tx.send(
                    Printout { verbosity: 2, content: "eth: got network error".to_string() }
                ).await;
                // if we hold active subscriptions for the remote node that this error refers to,
                // close them here -- they will need to resubscribe
                if let Some(sub_map) = active_subscriptions.get(&wrapped_error.source) {
                    for (_sub_id, sub) in sub_map.iter() {
                        if let ActiveSub::Local(handle) = sub {
                            let _ = print_tx.send(
                                Printout {
                                    verbosity: 2,
                                    content: "eth: closing remote sub in response to network error".to_string()
                                }).await;
                            handle.abort();
                        }
                    }
                }
                // we got an error from a remote node provider --
                // forward it to response channel if it exists
                if let Some(chan) = response_channels.get(&wrapped_error.id) {
                    // can't close channel here, as response may be an error
                    // and fulfill_request may wish to try other providers.
                    let _ = chan.send(Err(wrapped_error)).await;
                }
            }
            Some(km) = recv_in_client.recv() => {
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
                    &mut response_channels,
                )
                .await
                {
                    error_message(&our, km_id, response_target, e, &send_to_loop).await;
                };
            }
        }
    }
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
    response_channels: &mut ResponseChannels,
) -> Result<(), EthError> {
    println!("provider: handle_message\r");
    match &km.message {
        Message::Response(_) => {
            // map response to the correct channel
            if let Some(chan) = response_channels.get(&km.id) {
                // can't close channel here, as response may be an error
                // and fulfill_request may wish to try other providers.
                let _ = chan.send(Ok(km)).await;
            } else {
                println!("eth: got weird response!!\r");
            }
            Ok(())
        }
        Message::Request(req) => {
            let Some(timeout) = req.expects_response else {
                // if they don't want a response, we don't need to do anything
                // might as well throw it away
                return Err(EthError::MalformedRequest);
            };
            let Ok(req) = serde_json::from_slice::<IncomingReq>(&req.body) else {
                return Err(EthError::MalformedRequest);
            };
            match req {
                IncomingReq::EthAction(eth_action) => {
                    handle_eth_action(
                        our,
                        access_settings,
                        send_to_loop,
                        km,
                        timeout,
                        eth_action,
                        providers,
                        active_subscriptions,
                        response_channels,
                    )
                    .await
                }
                IncomingReq::EthConfigAction(eth_config_action) => {
                    kernel_message(
                        our,
                        km.id,
                        km.source.clone(),
                        km.rsvp.clone(),
                        false,
                        None,
                        handle_eth_config_action(
                            our,
                            access_settings,
                            caps_oracle,
                            &km,
                            eth_config_action,
                            providers,
                        )
                        .await,
                        send_to_loop,
                    )
                    .await;
                    Ok(())
                }
                IncomingReq::EthSubResult(eth_sub_result) => {
                    println!("eth: got eth_sub_result\r");
                    // forward this to rsvp, if we have the sub id in our active subs
                    let Some(rsvp) = km.rsvp else {
                        return Ok(()); // no rsvp, no need to forward
                    };
                    let sub_id = match &eth_sub_result {
                        Ok(EthSub { id, .. }) => id,
                        Err(EthSubError { id, .. }) => id,
                    };
                    if let Some(sub_map) = active_subscriptions.get(&rsvp) {
                        if let Some(sub) = sub_map.get(sub_id) {
                            if let ActiveSub::Remote(node_provider) = sub {
                                if node_provider == &km.source.node {
                                    kernel_message(
                                        our,
                                        km.id,
                                        rsvp,
                                        None,
                                        true,
                                        None,
                                        eth_sub_result,
                                        send_to_loop,
                                    )
                                    .await;
                                    return Ok(());
                                }
                            }
                        }
                    }
                    println!("eth: got eth_sub_result but no matching sub found\r");
                    Ok(())
                }
            }
        }
    }
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
    response_channels: &mut ResponseChannels,
) -> Result<(), EthError> {
    println!("provider: handle_eth_action: {eth_action:?}\r");
    println!("access settings: {access_settings:?}\r");
    // check our access settings if the request is from a remote node
    if km.source.node != our {
        if access_settings.deny.contains(&km.source.node) {
            return Err(EthError::PermissionDenied);
        }
        if !access_settings.public {
            if !access_settings.allow.contains(&km.source.node) {
                return Err(EthError::PermissionDenied);
            }
        }
    }

    // for each incoming action, we need to assign a provider from our map
    // based on the chain id. once we assign a provider, we can use it for
    // this request. if the provider is not usable, cycle through options
    // before returning an error.
    match eth_action {
        EthAction::SubscribeLogs { sub_id, .. } => {
            tokio::spawn(create_new_subscription(
                our.to_string(),
                km.id,
                km.source.clone(),
                km.rsvp,
                send_to_loop.clone(),
                sub_id,
                eth_action,
                providers.clone(),
                active_subscriptions.clone(),
                response_channels.clone(),
            ));
        }
        EthAction::UnsubscribeLogs(sub_id) => {
            let mut sub_map = active_subscriptions
                .entry(km.source)
                .or_insert(HashMap::new());
            if let Some(sub) = sub_map.remove(&sub_id) {
                match sub {
                    ActiveSub::Local(handle) => {
                        handle.abort();
                    }
                    ActiveSub::Remote(node) => {
                        kernel_message(
                            our,
                            rand::random(),
                            Address {
                                node: node.clone(),
                                process: ETH_PROCESS_ID.clone(),
                            },
                            None,
                            true,
                            Some(60), // TODO
                            serde_json::to_vec(&eth_action).unwrap(),
                            send_to_loop,
                        )
                        .await;
                    }
                }
            }
        }
        EthAction::Request { .. } => {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            response_channels.insert(km.id, sender);
            let our = our.to_string();
            let send_to_loop = send_to_loop.clone();
            let providers = providers.clone();
            let response_channels = response_channels.clone();
            tokio::spawn(async move {
                let res = tokio::time::timeout(
                    std::time::Duration::from_secs(timeout),
                    fulfill_request(&our, km.id, &send_to_loop, eth_action, providers, receiver),
                )
                .await;
                match res {
                    Ok(Ok(response)) => {
                        kernel_message(
                            &our,
                            km.id,
                            km.source,
                            km.rsvp,
                            false,
                            None,
                            response,
                            &send_to_loop,
                        )
                        .await;
                    }
                    Ok(Err(e)) => {
                        error_message(&our, km.id, km.source, e, &send_to_loop).await;
                    }
                    Err(_) => {
                        error_message(&our, km.id, km.source, EthError::RpcTimeout, &send_to_loop)
                            .await;
                    }
                }
                response_channels.remove(&km.id);
            });
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
    sub_id: u64,
    eth_action: EthAction,
    providers: Providers,
    active_subscriptions: ActiveSubscriptions,
    response_channels: ResponseChannels,
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
        response_channels.clone(),
    )
    .await
    {
        Ok((Some(maintain_subscription), None)) => {
            // this is a local sub, as in, we connect to the rpc endpt
            // send a response to the target that the subscription was successful
            kernel_message(
                &our,
                km_id,
                target.clone(),
                rsvp.clone(),
                false,
                None,
                EthResponse::Ok,
                &send_to_loop,
            )
            .await;
            let mut subs = active_subscriptions
                .entry(target.clone())
                .or_insert(HashMap::new());
            let active_subscriptions = active_subscriptions.clone();
            subs.insert(
                sub_id,
                ActiveSub::Local(tokio::spawn(async move {
                    // await the subscription error and kill it if so
                    if let Err(e) = maintain_subscription.await {
                        error_message(&our, km_id, target.clone(), e, &send_to_loop).await;
                        active_subscriptions.entry(target).and_modify(|sub_map| {
                            sub_map.remove(&km_id);
                        });
                    }
                })),
            );
        }
        Ok((None, Some(provider_node))) => {
            // this is a remote sub
            let mut subs = active_subscriptions
                .entry(target.clone())
                .or_insert(HashMap::new());
            subs.insert(sub_id, ActiveSub::Remote(provider_node));
        }
        Err(e) => {
            error_message(&our, km_id, target.clone(), e, &send_to_loop).await;
        }
        _ => panic!(),
    }
}

async fn build_subscription(
    our: String,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    send_to_loop: MessageSender,
    eth_action: &EthAction,
    providers: Providers,
    response_channels: ResponseChannels,
) -> Result<
    (
        // this is dumb, sorry
        Option<impl Future<Output = Result<(), EthError>>>,
        Option<String>,
    ),
    EthError,
> {
    println!("provider: build_subscription\r");
    let EthAction::SubscribeLogs {
        sub_id,
        chain_id,
        kind,
        params,
    } = eth_action
    else {
        return Err(EthError::PermissionDenied); // will never hit
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
            return Ok((
                Some(maintain_subscription(
                    our,
                    *sub_id,
                    rx,
                    target,
                    rsvp,
                    send_to_loop,
                )),
                None,
            ));
        }
        // this provider failed and needs to be reset
        url_provider.pubsub = None;
    }
    // now we need a response channel
    let (sender, mut response_receiver) = tokio::sync::mpsc::channel(1);
    response_channels.insert(km_id, sender);
    for node_provider in &mut aps.nodes {
        if !node_provider.usable {
            continue;
        }
        // in order, forward the request to each node provider
        // until one sends back a satisfactory response
        kernel_message(
            &our,
            km_id,
            Address {
                node: node_provider.name.clone(),
                process: ETH_PROCESS_ID.clone(),
            },
            rsvp.clone(),
            true,
            Some(60), // TODO
            eth_action,
            &send_to_loop,
        )
        .await;
        let Some(Ok(response_km)) = response_receiver.recv().await else {
            // our message timed out or receiver was offline
            println!("provider: build_subscription: response_receiver timed out / is offline\r");
            continue;
        };
        let Message::Response((resp, _context)) = response_km.message else {
            // if we hit this, they spoofed a request with same id, ignore and possibly punish
            node_provider.usable = false;
            continue;
        };
        let Ok(eth_response) = serde_json::from_slice::<EthResponse>(&resp.body) else {
            // if we hit this, they sent a malformed response, ignore and possibly punish
            node_provider.usable = false;
            continue;
        };
        if let EthResponse::Response { .. } = &eth_response {
            // if we hit this, they sent a response instead of a subscription, ignore and possibly punish
            node_provider.usable = false;
            continue;
        }
        if let EthResponse::Err(_error) = &eth_response {
            // if we hit this, they sent an error, if it's an error that might
            // not be our fault, we can try another provider
            continue;
        }
        kernel_message(
            &our,
            km_id,
            target,
            None,
            false,
            None,
            EthResponse::Ok,
            &send_to_loop,
        )
        .await;
        response_channels.remove(&km_id);
        return Ok((None, Some(node_provider.name.clone())));
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
        let value = rx
            .recv()
            .await
            .map_err(|_| EthError::SubscriptionClosed(sub_id))?;
        let result: SubscriptionResult =
            serde_json::from_str(value.get()).map_err(|_| EthError::SubscriptionClosed(sub_id))?;
        kernel_message(
            &our,
            rand::random(),
            target.clone(),
            rsvp.clone(),
            true,
            None,
            EthSubResult::Ok(EthSub { id: sub_id, result }),
            &send_to_loop,
        )
        .await;
    }
}

async fn fulfill_request(
    our: &str,
    km_id: u64,
    send_to_loop: &MessageSender,
    eth_action: EthAction,
    providers: Providers,
    mut remote_request_receiver: ProcessMessageReceiver,
) -> Result<EthResponse, EthError> {
    println!("provider: fulfill_request\r");
    let EthAction::Request {
        chain_id,
        ref method,
        ref params,
    } = eth_action
    else {
        return Err(EthError::PermissionDenied); // will never hit
    };
    let Some(method) = to_static_str(&method) else {
        return Err(EthError::InvalidMethod(method.to_string()));
    };
    let Some(mut aps) = providers.get_mut(&chain_id) else {
        return Err(EthError::NoRpcForChain);
    };
    // first, try any url providers we have for this chain,
    // then if we have none or they all fail, go to node provider.
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
        let Ok(value) = pubsub.inner().prepare(method, params.clone()).await else {
            // this provider failed and needs to be reset
            url_provider.pubsub = None;
            continue;
        };
        return Ok(EthResponse::Response { value });
    }
    for node_provider in &mut aps.nodes {
        if !node_provider.usable || node_provider.name == our {
            continue;
        }
        // in order, forward the request to each node provider
        // until one sends back a satisfactory response
        kernel_message(
            our,
            km_id,
            Address {
                node: node_provider.name.clone(),
                process: ETH_PROCESS_ID.clone(),
            },
            None,
            true,
            Some(60), // TODO
            eth_action.clone(),
            &send_to_loop,
        )
        .await;
        let Some(Ok(response_km)) = remote_request_receiver.recv().await else {
            println!("provider: fulfill_request: remote_request_receiver timed out / is offline\r");
            continue;
        };
        let Message::Response((resp, _context)) = response_km.message else {
            // if we hit this, they spoofed a request with same id, ignore and possibly punish
            node_provider.usable = false;
            continue;
        };
        let Ok(eth_response) = serde_json::from_slice::<EthResponse>(&resp.body) else {
            // if we hit this, they sent a malformed response, ignore and possibly punish
            node_provider.usable = false;
            continue;
        };
        if let EthResponse::Err(error) = &eth_response {
            // if we hit this, they sent an error, if it's an error that might
            // not be our fault, we can try another provider
            match error {
                EthError::NoRpcForChain => continue,
                EthError::PermissionDenied => continue,
                _ => {}
            }
        }
        return Ok(eth_response);
    }
    Err(EthError::NoRpcForChain)
}

async fn handle_eth_config_action(
    our: &str,
    access_settings: &mut AccessSettings,
    caps_oracle: &CapMessageSender,
    km: &KernelMessage,
    eth_config_action: EthConfigAction,
    providers: &mut Providers,
) -> EthConfigResponse {
    println!("provider: handle_eth_config_action\r");
    if km.source.node != our {
        return EthConfigResponse::PermissionDenied;
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
        println!("eth: capability oracle denied request, no cap\r");
        return EthConfigResponse::PermissionDenied;
    }
    println!("cap valid\r");

    // modify our providers and access settings based on config action
    match eth_config_action {
        EthConfigAction::AddProvider(provider) => {
            let mut aps = providers
                .entry(provider.chain_id)
                .or_insert(ActiveProviders {
                    urls: vec![],
                    nodes: vec![],
                });
            aps.add_provider_config(provider);
        }
        EthConfigAction::RemoveProvider((chain_id, remove)) => {
            if let Some(mut aps) = providers.get_mut(&chain_id) {
                aps.remove_provider(&remove);
            }
        }
        EthConfigAction::SetPublic => {
            println!("set public\r");
            access_settings.public = true;
        }
        EthConfigAction::SetPrivate => {
            println!("set private\r");
            access_settings.public = false;
        }
        EthConfigAction::AllowNode(node) => {
            access_settings.allow.insert(node);
        }
        EthConfigAction::UnallowNode(node) => {
            access_settings.allow.remove(&node);
        }
        EthConfigAction::DenyNode(node) => {
            access_settings.deny.insert(node);
        }
        EthConfigAction::UndenyNode(node) => {
            access_settings.deny.remove(&node);
        }
        EthConfigAction::SetProviders(new_providers) => {
            let new_map = DashMap::new();
            for entry in new_providers {
                let mut aps = new_map.entry(entry.chain_id).or_insert(ActiveProviders {
                    urls: vec![],
                    nodes: vec![],
                });
                aps.add_provider_config(entry);
            }
            *providers = Arc::new(new_map);
        }
        EthConfigAction::GetProviders => {
            return EthConfigResponse::Providers(
                providers
                    .iter()
                    .map(|entry| {
                        entry
                            .urls
                            .iter()
                            .map(|url_provider| ProviderConfig {
                                chain_id: *entry.key(),
                                provider: NodeOrRpcUrl::RpcUrl(url_provider.url.clone()),
                                trusted: url_provider.trusted,
                            })
                            .chain(entry.nodes.iter().map(|node_provider| ProviderConfig {
                                chain_id: *entry.key(),
                                provider: NodeOrRpcUrl::Node {
                                    kns_update: KnsUpdate {
                                        name: node_provider.name.clone(),
                                        owner: "".to_string(),
                                        node: "".to_string(),
                                        public_key: "".to_string(),
                                        ip: "".to_string(),
                                        port: 0,
                                        routers: vec![],
                                    },
                                    use_as_provider: node_provider.usable,
                                },
                                trusted: node_provider.trusted,
                            }))
                            .collect::<Vec<_>>()
                    })
                    .flatten()
                    .collect(),
            );
        }
        EthConfigAction::GetAccessSettings => {
            return EthConfigResponse::AccessSettings(access_settings.clone());
        }
    }
    EthConfigResponse::Ok
}

async fn error_message(
    our: &str,
    km_id: u64,
    target: Address,
    error: EthError,
    send_to_loop: &MessageSender,
) {
    println!("EthError: {error:?}\r");
    kernel_message(
        our,
        km_id,
        target,
        None,
        false,
        None,
        EthResponse::Err(error),
        send_to_loop,
    )
    .await
}

async fn kernel_message<T: Serialize>(
    our: &str,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    req: bool,
    timeout: Option<u64>,
    body: T,
    send_to_loop: &MessageSender,
) {
    let _ = send_to_loop
        .send(KernelMessage {
            id: km_id,
            source: Address {
                node: our.to_string(),
                process: ETH_PROCESS_ID.clone(),
            },
            target,
            rsvp,
            message: if req {
                Message::Request(Request {
                    inherit: false,
                    expects_response: timeout,
                    body: serde_json::to_vec(&body).unwrap(),
                    metadata: None,
                    capabilities: vec![],
                })
            } else {
                Message::Response((
                    Response {
                        inherit: false,
                        body: serde_json::to_vec(&body).unwrap(),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                ))
            },
            lazy_load_blob: None,
        })
        .await;
}
