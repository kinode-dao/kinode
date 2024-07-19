use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::client::WsConnect;
use alloy::rpc::json_rpc::RpcError;
use anyhow::Result;
use dashmap::DashMap;
use lib::types::core::*;
use lib::types::eth::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::task::JoinHandle;
use url::Url;

mod subscription;

/// meta-type for all incoming requests we need to handle
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum IncomingReq {
    /// requests for an RPC action that can come from processes on this node or others
    EthAction(EthAction),
    /// requests that must come from this node to modify provider settings / fetch them
    EthConfigAction(EthConfigAction),
    /// subscription updates coming in from a remote provider
    EthSubResult(EthSubResult),
    /// a remote node who uses our provider keeping their subscription alive
    SubKeepalive(u64),
}

/// mapping of chain id to ordered lists of providers
type Providers = Arc<DashMap<u64, ActiveProviders>>;

#[derive(Debug)]
struct ActiveProviders {
    pub urls: Vec<UrlProvider>,
    pub nodes: Vec<NodeProvider>,
}

#[derive(Debug, Clone)]
struct UrlProvider {
    pub trusted: bool,
    pub url: String,
    pub pubsub: Option<RootProvider<PubSubFrontend>>,
}

#[derive(Debug, Clone)]
struct NodeProvider {
    /// NOT CURRENTLY USED
    pub trusted: bool,
    /// semi-temporary flag to mark if this provider is currently usable
    /// future updates will make this more dynamic
    pub usable: bool,
    /// the KNS update that describes this node provider
    /// kept so we can re-serialize to SavedConfigs
    pub kns_update: KnsUpdate,
}

impl ActiveProviders {
    fn add_provider_config(&mut self, new: ProviderConfig) {
        match new.provider {
            NodeOrRpcUrl::Node {
                kns_update,
                use_as_provider,
            } => {
                self.remove_provider(&kns_update.name);
                self.nodes.insert(
                    0,
                    NodeProvider {
                        trusted: new.trusted,
                        usable: use_as_provider,
                        kns_update,
                    },
                );
            }
            NodeOrRpcUrl::RpcUrl(url) => {
                self.remove_provider(&url);
                self.urls.insert(
                    0,
                    UrlProvider {
                        trusted: new.trusted,
                        url,
                        pubsub: None,
                    },
                );
            }
        }
    }

    fn remove_provider(&mut self, remove: &str) {
        self.urls.retain(|x| x.url != remove);
        self.nodes.retain(|x| x.kns_update.name != remove);
    }
}

/// existing subscriptions held by local OR remote processes
type ActiveSubscriptions = Arc<DashMap<Address, HashMap<u64, ActiveSub>>>;

type ResponseChannels = Arc<DashMap<u64, ProcessMessageSender>>;

#[derive(Debug)]
enum ActiveSub {
    Local(JoinHandle<()>),
    Remote {
        provider_node: String,
        handle: JoinHandle<()>,
        sender: tokio::sync::mpsc::Sender<EthSubResult>,
    },
}

impl ActiveSub {
    async fn close(&self, sub_id: u64, state: &ModuleState) {
        match self {
            ActiveSub::Local(handle) => {
                handle.abort();
            }
            ActiveSub::Remote {
                provider_node,
                handle,
                ..
            } => {
                // tell provider node we don't need their services anymore
                kernel_message(
                    &state.our,
                    rand::random(),
                    Address {
                        node: provider_node.clone(),
                        process: ETH_PROCESS_ID.clone(),
                    },
                    None,
                    true,
                    None,
                    EthAction::UnsubscribeLogs(sub_id),
                    &state.send_to_loop,
                )
                .await;
                handle.abort();
            }
        }
    }
}

struct ModuleState {
    /// the name of this node
    our: Arc<String>,
    /// the home directory path
    home_directory_path: String,
    /// the access settings for this provider
    access_settings: AccessSettings,
    /// the set of providers we have available for all chains
    providers: Providers,
    /// the set of active subscriptions we are currently maintaining
    active_subscriptions: ActiveSubscriptions,
    /// the set of response channels we have open for outstanding request tasks
    response_channels: ResponseChannels,
    /// our sender for kernel event loop
    send_to_loop: MessageSender,
    /// our sender for terminal prints
    print_tx: PrintSender,
}

/// The ETH provider runtime process is responsible for connecting to one or more ETH RPC providers
/// and using them to service indexing requests from other apps. This is the runtime entry point
/// for the entire module.
pub async fn provider(
    our: String,
    home_directory_path: String,
    configs: SavedConfigs,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    mut net_error_recv: NetworkErrorReceiver,
    caps_oracle: CapMessageSender,
    print_tx: PrintSender,
) -> Result<()> {
    // load access settings if they've been persisted to disk
    // this merely describes whether our provider is available to other nodes
    // and if so, which nodes are allowed to access it (public/whitelist/blacklist)
    let access_settings: AccessSettings =
        match tokio::fs::read_to_string(format!("{}/.eth_access_settings", home_directory_path))
            .await
        {
            Ok(contents) => serde_json::from_str(&contents).unwrap(),
            Err(_) => {
                let access_settings = AccessSettings {
                    public: false,
                    allow: HashSet::new(),
                    deny: HashSet::new(),
                };
                access_settings
            }
        };
    verbose_print(
        &print_tx,
        &format!("eth: access settings loaded: {access_settings:?}"),
    )
    .await;

    // initialize module state
    // fill out providers based on saved configs (possibly persisted, given to us)
    // this can be a mix of node providers and rpc providers
    let mut state = ModuleState {
        our: Arc::new(our),
        home_directory_path,
        access_settings,
        providers: Arc::new(DashMap::new()),
        active_subscriptions: Arc::new(DashMap::new()),
        response_channels: Arc::new(DashMap::new()),
        send_to_loop,
        print_tx,
    };

    // convert saved configs into data structure that we will use to route queries
    for entry in configs {
        let mut ap = state
            .providers
            .entry(entry.chain_id)
            .or_insert(ActiveProviders {
                urls: vec![],
                nodes: vec![],
            });
        ap.add_provider_config(entry);
    }

    verbose_print(&state.print_tx, "eth: provider initialized").await;

    // main loop: handle incoming network errors and incoming kernel messages
    loop {
        tokio::select! {
            Some(wrapped_error) = net_error_recv.recv() => {
                handle_network_error(
                    wrapped_error,
                    &state,
                ).await;
            }
            Some(km) = recv_in_client.recv() => {
                let km_id = km.id;
                let response_target = km.rsvp.as_ref().unwrap_or(&km.source).clone();
                if let Err(e) = handle_message(
                    &mut state,
                    km,
                    &caps_oracle,
                )
                .await
                {
                    error_message(
                        &state.our,
                        km_id,
                        response_target,
                        e,
                        &state.send_to_loop
                    ).await;
                };
            }
        }
    }
}

/// network errors only come from remote provider nodes we tried to access,
/// or from remote nodes that are using us as a provider.
///
/// if we tried to access them, we will have a response channel to send the error to.
/// if they are using us as a provider, close the subscription associated with the target.
async fn handle_network_error(wrapped_error: WrappedSendError, state: &ModuleState) {
    verbose_print(
        &state.print_tx,
        &format!(
            "eth: got network error from {}",
            &wrapped_error.error.target
        ),
    )
    .await;

    // close all subscriptions held by the process that we (possibly) tried to send an update to
    if let Some((_who, sub_map)) = state
        .active_subscriptions
        .remove(&wrapped_error.error.target)
    {
        for (sub_id, sub) in sub_map.iter() {
            verbose_print(
                &state.print_tx,
                &format!(
                    "eth: closed subscription {} in response to network error",
                    sub_id
                ),
            )
            .await;
            sub.close(*sub_id, state).await;
        }
    }

    // forward error to response channel if it exists
    if let Some(chan) = state.response_channels.get(&wrapped_error.id) {
        // don't close channel here, as channel holder will wish to try other providers.
        verbose_print(
            &state.print_tx,
            "eth: forwarded network error to response channel",
        )
        .await;
        let _ = chan.send(Err(wrapped_error)).await;
    }
}

/// handle incoming requests and responses.
/// requests must be one of types in [`IncomingReq`].
/// responses are passthroughs from remote provider nodes.
async fn handle_message(
    state: &mut ModuleState,
    km: KernelMessage,
    caps_oracle: &CapMessageSender,
) -> Result<(), EthError> {
    match &km.message {
        Message::Response(_) => {
            // map response to the correct channel
            if let Some(chan) = state.response_channels.get(&km.id) {
                // can't close channel here, as response may be an error
                // and fulfill_request may wish to try other providers.
                let _ = chan.send(Ok(km)).await;
            } else {
                verbose_print(
                    &state.print_tx,
                    "eth: got response but no matching channel found",
                )
                .await;
            }
        }
        Message::Request(req) => {
            let timeout = req.expects_response.unwrap_or(60);
            let Ok(req) = serde_json::from_slice::<IncomingReq>(&req.body) else {
                return Err(EthError::MalformedRequest);
            };
            match req {
                IncomingReq::EthAction(eth_action) => {
                    return handle_eth_action(state, km, timeout, eth_action).await;
                }
                IncomingReq::EthConfigAction(eth_config_action) => {
                    kernel_message(
                        &state.our.clone(),
                        km.id,
                        km.rsvp.as_ref().unwrap_or(&km.source).clone(),
                        None,
                        false,
                        None,
                        handle_eth_config_action(state, caps_oracle, &km, eth_config_action).await,
                        &state.send_to_loop,
                    )
                    .await;
                }
                IncomingReq::EthSubResult(eth_sub_result) => {
                    // forward this to rsvp, if we have the sub id in our active subs
                    let Some(rsvp) = km.rsvp else {
                        return Ok(()); // no rsvp, no need to forward
                    };
                    let sub_id = match eth_sub_result {
                        Ok(EthSub { id, .. }) => id,
                        Err(EthSubError { id, .. }) => id,
                    };
                    if let Some(sub_map) = state.active_subscriptions.get(&rsvp) {
                        if let Some(ActiveSub::Remote {
                            provider_node,
                            sender,
                            ..
                        }) = sub_map.get(&sub_id)
                        {
                            if provider_node == &km.source.node {
                                if let Ok(()) = sender.send(eth_sub_result).await {
                                    // successfully sent a subscription update from a
                                    // remote provider to one of our processes
                                    return Ok(());
                                }
                            }
                        }
                    }
                    // tell the remote provider that we don't have this sub
                    // so they can stop sending us updates
                    verbose_print(
                        &state.print_tx,
                        "eth: got eth_sub_result but no matching sub found, unsubscribing",
                    )
                    .await;
                    kernel_message(
                        &state.our.clone(),
                        km.id,
                        km.source.clone(),
                        None,
                        true,
                        None,
                        EthAction::UnsubscribeLogs(sub_id),
                        &state.send_to_loop,
                    )
                    .await;
                }
                IncomingReq::SubKeepalive(sub_id) => {
                    // source expects that we have a local sub for them with this id
                    // if we do, no action required, otherwise, throw them an error.
                    if let Some(sub_map) = state.active_subscriptions.get(&km.source) {
                        if sub_map.contains_key(&sub_id) {
                            return Ok(());
                        }
                    }
                    verbose_print(
                        &state.print_tx,
                        &format!(
                            "eth: got sub_keepalive from {} but no matching sub found",
                            km.source
                        ),
                    )
                    .await;
                    // send a response with an EthSubError
                    kernel_message(
                        &state.our.clone(),
                        km.id,
                        km.source.clone(),
                        None,
                        false,
                        None,
                        EthSubResult::Err(EthSubError {
                            id: sub_id,
                            error: "Subscription not found".to_string(),
                        }),
                        &state.send_to_loop,
                    )
                    .await;
                }
            }
        }
    }
    Ok(())
}

async fn handle_eth_action(
    state: &mut ModuleState,
    km: KernelMessage,
    timeout: u64,
    eth_action: EthAction,
) -> Result<(), EthError> {
    // check our access settings if the request is from a remote node
    if km.source.node != *state.our {
        if state.access_settings.deny.contains(&km.source.node)
            || (!state.access_settings.public
                && !state.access_settings.allow.contains(&km.source.node))
        {
            verbose_print(
                &state.print_tx,
                "eth: got eth_action from unauthorized remote source",
            )
            .await;
            return Err(EthError::PermissionDenied);
        }
    }

    verbose_print(
        &state.print_tx,
        &format!(
            "eth: handling {} from {}",
            match &eth_action {
                EthAction::SubscribeLogs { .. } => "subscribe",
                EthAction::UnsubscribeLogs(_) => "unsubscribe",
                EthAction::Request { .. } => "request",
            },
            km.source
        ),
    )
    .await;

    // for each incoming action, we need to assign a provider from our map
    // based on the chain id. once we assign a provider, we can use it for
    // this request. if the provider is not usable, cycle through options
    // before returning an error.
    match eth_action {
        EthAction::SubscribeLogs { sub_id, .. } => {
            subscription::create_new_subscription(
                state,
                km.id,
                km.source.clone(),
                km.rsvp,
                sub_id,
                eth_action,
            )
            .await;
        }
        EthAction::UnsubscribeLogs(sub_id) => {
            let mut sub_map = state
                .active_subscriptions
                .entry(km.source.clone())
                .or_insert(HashMap::new());
            if let Some(sub) = sub_map.remove(&sub_id) {
                sub.close(sub_id, state).await;
                kernel_message(
                    &state.our,
                    km.id,
                    km.rsvp.unwrap_or(km.source),
                    None,
                    false,
                    None,
                    EthResponse::Ok,
                    &state.send_to_loop,
                )
                .await;
            } else {
                verbose_print(
                    &state.print_tx,
                    "eth: got unsubscribe but no matching subscription found",
                )
                .await;
                error_message(
                    &state.our,
                    km.id,
                    km.source,
                    EthError::MalformedRequest,
                    &state.send_to_loop,
                )
                .await;
            }
        }
        EthAction::Request { .. } => {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            state.response_channels.insert(km.id, sender);
            let our = state.our.to_string();
            let send_to_loop = state.send_to_loop.clone();
            let providers = state.providers.clone();
            let response_channels = state.response_channels.clone();
            let print_tx = state.print_tx.clone();
            tokio::spawn(async move {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(timeout),
                    fulfill_request(
                        &our,
                        km.id,
                        &send_to_loop,
                        eth_action,
                        providers,
                        receiver,
                        &print_tx,
                    ),
                )
                .await
                {
                    Ok(response) => {
                        kernel_message(
                            &our,
                            km.id,
                            km.rsvp.unwrap_or(km.source),
                            None,
                            false,
                            None,
                            response,
                            &send_to_loop,
                        )
                        .await;
                    }
                    Err(_) => {
                        // task timeout
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

async fn fulfill_request(
    our: &str,
    km_id: u64,
    send_to_loop: &MessageSender,
    eth_action: EthAction,
    providers: Providers,
    mut remote_request_receiver: ProcessMessageReceiver,
    print_tx: &PrintSender,
) -> EthResponse {
    let EthAction::Request {
        chain_id,
        ref method,
        ref params,
    } = eth_action
    else {
        return EthResponse::Err(EthError::PermissionDenied); // will never hit
    };
    let Some(method) = to_static_str(&method) else {
        return EthResponse::Err(EthError::InvalidMethod(method.to_string()));
    };
    let mut urls = {
        // in code block to drop providers lock asap to avoid deadlock
        let Some(aps) = providers.get(&chain_id) else {
            return EthResponse::Err(EthError::NoRpcForChain);
        };
        aps.urls.clone()
    };

    // first, try any url providers we have for this chain,
    // then if we have none or they all fail, go to node providers.
    // finally, if no provider works, return an error.

    // bump the successful provider to the front of the list for future requests
    for url_provider in urls.iter_mut() {
        let pubsub = match &url_provider.pubsub {
            Some(pubsub) => pubsub,
            None => {
                if let Ok(()) = activate_url_provider(url_provider).await {
                    verbose_print(
                        print_tx,
                        &format!("eth: activated url provider {}", url_provider.url),
                    )
                    .await;
                    url_provider.pubsub.as_ref().unwrap()
                } else {
                    verbose_print(
                        print_tx,
                        &format!("eth: could not activate url provider {}", url_provider.url),
                    )
                    .await;
                    continue;
                }
            }
        };
        match pubsub.raw_request(method.into(), params.clone()).await {
            Ok(value) => {
                let mut is_replacement_successful = true;
                providers.entry(chain_id).and_modify(|aps| {
                    let Some(index) = find_index(
                        &aps.urls.iter().map(|u| u.url.as_str()).collect(),
                        &url_provider.url,
                    ) else {
                        is_replacement_successful = false;
                        return ();
                    };
                    let old_provider = aps.urls.remove(index);
                    match old_provider.pubsub {
                        None => aps.urls.insert(0, url_provider.clone()),
                        Some(_) => aps.urls.insert(0, old_provider),
                    }
                });
                if !is_replacement_successful {
                    verbose_print(
                        print_tx,
                        &format!("eth: unexpectedly couldn't find provider to be modified"),
                    )
                    .await;
                }
                return EthResponse::Response { value };
            }
            Err(rpc_error) => {
                verbose_print(
                    print_tx,
                    &format!(
                        "eth: got error from url provider {}: {}",
                        url_provider.url, rpc_error
                    ),
                )
                .await;
                // if rpc_error is of type ErrResponse, return to user!
                if let RpcError::ErrorResp(err) = rpc_error {
                    return EthResponse::Err(EthError::RpcError(err));
                }
                // this provider failed and needs to be reset
                let mut is_reset_successful = true;
                providers.entry(chain_id).and_modify(|aps| {
                    let Some(index) = find_index(
                        &aps.urls.iter().map(|u| u.url.as_str()).collect(),
                        &url_provider.url,
                    ) else {
                        is_reset_successful = false;
                        return ();
                    };
                    let mut url = aps.urls.remove(index);
                    url.pubsub = None;
                    aps.urls.insert(index, url);
                });
                if !is_reset_successful {
                    verbose_print(
                        print_tx,
                        &format!("eth: unexpectedly couldn't find provider to be modified"),
                    )
                    .await;
                }
            }
        }
    }

    let nodes = {
        // in code block to drop providers lock asap to avoid deadlock
        let Some(aps) = providers.get(&chain_id) else {
            return EthResponse::Err(EthError::NoRpcForChain);
        };
        aps.nodes.clone()
    };
    for node_provider in &nodes {
        verbose_print(
            print_tx,
            &format!(
                "eth: attempting to fulfill via {}",
                node_provider.kns_update.name
            ),
        )
        .await;
        let response = forward_to_node_provider(
            our,
            km_id,
            None,
            node_provider,
            eth_action.clone(),
            send_to_loop,
            &mut remote_request_receiver,
        )
        .await;
        if let EthResponse::Err(e) = response {
            if let EthError::RpcMalformedResponse = e {
                set_node_unusable(
                    &providers,
                    &chain_id,
                    &node_provider.kns_update.name,
                    print_tx,
                )
                .await;
            }
        } else {
            return response;
        }
    }
    EthResponse::Err(EthError::NoRpcForChain)
}

/// take an EthAction and send it to a node provider, then await a response.
async fn forward_to_node_provider(
    our: &str,
    km_id: u64,
    rsvp: Option<Address>,
    node_provider: &NodeProvider,
    eth_action: EthAction,
    send_to_loop: &MessageSender,
    receiver: &mut ProcessMessageReceiver,
) -> EthResponse {
    if !node_provider.usable || node_provider.kns_update.name == our {
        return EthResponse::Err(EthError::PermissionDenied);
    }
    kernel_message(
        our,
        km_id,
        Address {
            node: node_provider.kns_update.name.clone(),
            process: ETH_PROCESS_ID.clone(),
        },
        rsvp,
        true,
        Some(60), // TODO
        eth_action.clone(),
        &send_to_loop,
    )
    .await;
    let Ok(Some(Ok(response_km))) =
        tokio::time::timeout(std::time::Duration::from_secs(30), receiver.recv()).await
    else {
        return EthResponse::Err(EthError::RpcTimeout);
    };
    if let Message::Response((resp, _context)) = response_km.message {
        if let Ok(eth_response) = serde_json::from_slice::<EthResponse>(&resp.body) {
            return eth_response;
        }
    }
    // if we hit this, they sent a malformed response, ignore and possibly punish
    EthResponse::Err(EthError::RpcMalformedResponse)
}

async fn handle_eth_config_action(
    state: &mut ModuleState,
    caps_oracle: &CapMessageSender,
    km: &KernelMessage,
    eth_config_action: EthConfigAction,
) -> EthConfigResponse {
    if km.source.node != *state.our {
        verbose_print(
            &state.print_tx,
            "eth: got eth_config_action from unauthorized remote source",
        )
        .await;
        return EthConfigResponse::PermissionDenied;
    }

    // check capabilities to ensure the sender is allowed to make this request
    if !check_for_root_cap(&state.our, &km.source.process, caps_oracle).await {
        verbose_print(
            &state.print_tx,
            "eth: got eth_config_action from unauthorized local source",
        )
        .await;
        return EthConfigResponse::PermissionDenied;
    }

    verbose_print(
        &state.print_tx,
        &format!("eth: handling eth_config_action {eth_config_action:?}"),
    )
    .await;

    let mut save_settings = false;
    let mut save_providers = false;

    // modify our providers and access settings based on config action
    match eth_config_action {
        EthConfigAction::AddProvider(provider) => {
            let mut aps = state
                .providers
                .entry(provider.chain_id)
                .or_insert(ActiveProviders {
                    urls: vec![],
                    nodes: vec![],
                });
            aps.add_provider_config(provider);
            save_providers = true;
        }
        EthConfigAction::RemoveProvider((chain_id, remove)) => {
            if let Some(mut aps) = state.providers.get_mut(&chain_id) {
                aps.remove_provider(&remove);
                save_providers = true;
            }
        }
        EthConfigAction::SetPublic => {
            state.access_settings.public = true;
            save_settings = true;
        }
        EthConfigAction::SetPrivate => {
            state.access_settings.public = false;
            save_settings = true;
        }
        EthConfigAction::AllowNode(node) => {
            state.access_settings.allow.insert(node);
            save_settings = true;
        }
        EthConfigAction::UnallowNode(node) => {
            state.access_settings.allow.remove(&node);
            save_settings = true;
        }
        EthConfigAction::DenyNode(node) => {
            state.access_settings.deny.insert(node);
            save_settings = true;
        }
        EthConfigAction::UndenyNode(node) => {
            state.access_settings.deny.remove(&node);
            save_settings = true;
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
            state.providers = Arc::new(new_map);
            save_providers = true;
        }
        EthConfigAction::GetProviders => {
            return EthConfigResponse::Providers(providers_to_saved_configs(&state.providers));
        }
        EthConfigAction::GetAccessSettings => {
            return EthConfigResponse::AccessSettings(state.access_settings.clone());
        }
        EthConfigAction::GetState => {
            return EthConfigResponse::State {
                active_subscriptions: state
                    .active_subscriptions
                    .iter()
                    .map(|e| {
                        (
                            e.key().clone(),
                            e.value()
                                .iter()
                                .map(|(id, sub)| {
                                    (
                                        *id,
                                        match sub {
                                            ActiveSub::Local(_) => None,
                                            ActiveSub::Remote { provider_node, .. } => {
                                                Some(provider_node.clone())
                                            }
                                        },
                                    )
                                })
                                .collect(),
                        )
                    })
                    .collect(),
                outstanding_requests: state.response_channels.iter().map(|e| *e.key()).collect(),
            };
        }
    }
    // save providers and/or access settings, depending on necessity, to disk
    if save_settings {
        if let Ok(()) = tokio::fs::write(
            format!("{}/.eth_access_settings", state.home_directory_path),
            serde_json::to_string(&state.access_settings).unwrap(),
        )
        .await
        {
            verbose_print(&state.print_tx, "eth: saved new access settings").await;
        };
    }
    if save_providers {
        if let Ok(()) = tokio::fs::write(
            format!("{}/.eth_providers", state.home_directory_path),
            serde_json::to_string(&providers_to_saved_configs(&state.providers)).unwrap(),
        )
        .await
        {
            verbose_print(&state.print_tx, "eth: saved new provider settings").await;
        };
    }
    EthConfigResponse::Ok
}

async fn activate_url_provider(provider: &mut UrlProvider) -> Result<()> {
    match Url::parse(&provider.url)?.scheme() {
        "ws" | "wss" => {
            let ws = WsConnect::new(provider.url.to_string());

            let client = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                ProviderBuilder::new().on_ws(ws),
            )
            .await??;
            provider.pubsub = Some(client);
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "Only `ws://` or `wss://` providers are supported."
        )),
    }
}

fn providers_to_saved_configs(providers: &Providers) -> SavedConfigs {
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
                        kns_update: node_provider.kns_update.clone(),
                        use_as_provider: node_provider.usable,
                    },
                    trusted: node_provider.trusted,
                }))
                .collect::<Vec<_>>()
        })
        .flatten()
        .collect()
}

async fn check_for_root_cap(
    our: &str,
    process: &ProcessId,
    caps_oracle: &CapMessageSender,
) -> bool {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    caps_oracle
        .send(CapMessage::Has {
            on: process.clone(),
            cap: Capability::new((our, ETH_PROCESS_ID.clone()), "{\"root\":true}"),
            responder: send_cap_bool,
        })
        .await
        .expect("eth: capability oracle died!");
    recv_cap_bool.await.unwrap_or(false)
}

async fn verbose_print(print_tx: &PrintSender, content: &str) {
    let _ = print_tx
        .send(Printout {
            verbosity: 2,
            content: content.to_string(),
        })
        .await;
}

async fn error_message(
    our: &str,
    km_id: u64,
    target: Address,
    error: EthError,
    send_to_loop: &MessageSender,
) {
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

fn find_index(vec: &Vec<&str>, item: &str) -> Option<usize> {
    vec.iter().enumerate().find_map(
        |(index, value)| {
            if *value == item {
                Some(index)
            } else {
                None
            }
        },
    )
}

async fn set_node_unusable(
    providers: &Providers,
    chain_id: &u64,
    node_name: &str,
    print_tx: &PrintSender,
) -> bool {
    let mut is_replacement_successful = true;
    providers.entry(chain_id.clone()).and_modify(|aps| {
        let Some(index) = find_index(
            &aps.nodes
                .iter()
                .map(|n| n.kns_update.name.as_str())
                .collect(),
            &node_name,
        ) else {
            is_replacement_successful = false;
            return ();
        };
        let mut node = aps.nodes.remove(index);
        node.usable = false;
        aps.nodes.insert(index, node);
    });
    if !is_replacement_successful {
        verbose_print(
            print_tx,
            &format!("eth: unexpectedly couldn't find provider to be modified"),
        )
        .await;
    }
    is_replacement_successful
}
