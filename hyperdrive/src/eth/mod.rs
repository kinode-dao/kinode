use alloy::providers::{Provider, RootProvider};
use alloy::pubsub::PubSubFrontend;
use alloy::rpc::json_rpc::RpcError;
use anyhow::Result;
use dashmap::DashMap;
use indexmap::IndexMap;
use lib::types::core::*;
use lib::types::eth::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use utils::*;

mod subscription;
mod utils;

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
    /// a list, in case we build multiple providers for the same url
    pub pubsub: Vec<RootProvider<PubSubFrontend>>,
    pub auth: Option<Authorization>,
}

#[derive(Debug, Clone)]
struct NodeProvider {
    /// NOT CURRENTLY USED
    pub trusted: bool,
    /// semi-temporary flag to mark if this provider is currently usable
    /// future updates will make this more dynamic
    pub usable: bool,
    /// the HNS update that describes this node provider
    /// kept so we can re-serialize to SavedConfigs
    pub hns_update: HnsUpdate,
}

impl ActiveProviders {
    fn add_provider_config(&mut self, new: ProviderConfig) {
        match new.provider {
            NodeOrRpcUrl::Node {
                hns_update,
                use_as_provider,
            } => {
                self.remove_provider(&hns_update.name);
                self.nodes.insert(
                    0,
                    NodeProvider {
                        trusted: new.trusted,
                        usable: use_as_provider,
                        hns_update,
                    },
                );
            }
            NodeOrRpcUrl::RpcUrl { url, auth } => {
                self.remove_provider(&url);
                self.urls.insert(
                    0,
                    UrlProvider {
                        trusted: new.trusted,
                        url,
                        pubsub: vec![],
                        auth,
                    },
                );
            }
        }
    }

    fn remove_provider(&mut self, remove: &str) {
        self.urls.retain(|x| x.url != remove);
        self.nodes.retain(|x| x.hns_update.name != remove);
    }
}

/// existing subscriptions held by local OR remote processes
type ActiveSubscriptions = Arc<DashMap<Address, HashMap<u64, ActiveSub>>>;

type ResponseChannels = Arc<DashMap<u64, ProcessMessageSender>>;

#[derive(Debug)]
enum ActiveSub {
    Local((tokio::sync::mpsc::Sender<bool>, JoinHandle<()>)),
    Remote {
        provider_node: String,
        handle: JoinHandle<()>,
        sender: tokio::sync::mpsc::Sender<EthSubResult>,
    },
}

impl ActiveSub {
    async fn close(&self, sub_id: u64, state: &ModuleState) {
        match self {
            ActiveSub::Local((close_sender, _handle)) => {
                close_sender.send(true).await.unwrap();
                //handle.abort();
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
    home_directory_path: PathBuf,
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
    /// cache of ETH requests
    request_cache: RequestCache,
}

type RequestCache = Arc<Mutex<IndexMap<Vec<u8>, (EthResponse, Instant)>>>;

const DELAY_MS: u64 = 1_000;
const MAX_REQUEST_CACHE_LEN: usize = 500;

/// TODO replace with alloy abstraction
fn valid_method(method: &str) -> Option<&'static str> {
    match method {
        "eth_getBalance" => Some("eth_getBalance"),
        "eth_sendRawTransaction" => Some("eth_sendRawTransaction"),
        "eth_call" => Some("eth_call"),
        "eth_chainId" => Some("eth_chainId"),
        "eth_getTransactionReceipt" => Some("eth_getTransactionReceipt"),
        "eth_getTransactionCount" => Some("eth_getTransactionCount"),
        "eth_estimateGas" => Some("eth_estimateGas"),
        "eth_blockNumber" => Some("eth_blockNumber"),
        "eth_getBlockByHash" => Some("eth_getBlockByHash"),
        "eth_getBlockByNumber" => Some("eth_getBlockByNumber"),
        "eth_getTransactionByHash" => Some("eth_getTransactionByHash"),
        "eth_getCode" => Some("eth_getCode"),
        "eth_getStorageAt" => Some("eth_getStorageAt"),
        "eth_gasPrice" => Some("eth_gasPrice"),
        "eth_accounts" => Some("eth_accounts"),
        "eth_hashrate" => Some("eth_hashrate"),
        "eth_getLogs" => Some("eth_getLogs"),
        "eth_subscribe" => Some("eth_subscribe"),
        "eth_unsubscribe" => Some("eth_unsubscribe"),
        // "eth_mining" => Some("eth_mining"),
        // "net_version" => Some("net_version"),
        // "net_peerCount" => Some("net_peerCount"),
        // "net_listening" => Some("net_listening"),
        // "web3_clientVersion" => Some("web3_clientVersion"),
        // "web3_sha3" => Some("web3_sha3"),
        _ => None,
    }
}

/// The ETH provider runtime process is responsible for connecting to one or more ETH RPC providers
/// and using them to service indexing requests from other apps. This is the runtime entry point
/// for the entire module.
pub async fn provider(
    our: String,
    home_directory_path: PathBuf,
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
        match tokio::fs::read_to_string(home_directory_path.join(".eth_access_settings")).await {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or(AccessSettings {
                public: false,
                allow: HashSet::new(),
                deny: HashSet::new(),
            }),
            Err(_) => AccessSettings {
                public: false,
                allow: HashSet::new(),
                deny: HashSet::new(),
            },
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
        request_cache: Arc::new(Mutex::new(IndexMap::new())),
    };

    // convert saved configs into data structure that we will use to route queries
    for entry in configs.0.into_iter().rev() {
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
            Ok(())
        }
        Message::Request(req) => {
            let timeout = req.expects_response.unwrap_or(60);
            let Ok(req) = serde_json::from_slice::<IncomingReq>(&req.body) else {
                return Err(EthError::MalformedRequest);
            };
            match req {
                IncomingReq::EthAction(eth_action) => {
                    handle_eth_action(state, km, timeout, eth_action).await
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
                    Ok(())
                }
                IncomingReq::EthSubResult(eth_sub_result) => {
                    // forward this to rsvp, if we have the sub id in our active subs
                    let Some(rsvp) = km.rsvp else {
                        verbose_print(
                            &state.print_tx,
                            "eth: got eth_sub_result with no rsvp, ignoring",
                        )
                        .await;
                        return Ok(()); // no rsvp, no need to forward
                    };
                    let sub_id = match eth_sub_result {
                        Ok(EthSub { id, .. }) => id,
                        Err(EthSubError { id, .. }) => id,
                    };
                    if let Some(mut sub_map) = state.active_subscriptions.get_mut(&rsvp) {
                        if let Some(sub) = sub_map.get(&sub_id) {
                            if let ActiveSub::Remote {
                                provider_node,
                                sender,
                                ..
                            } = sub
                            {
                                if provider_node == &km.source.node {
                                    if let Ok(()) = sender.send(eth_sub_result).await {
                                        // successfully sent a subscription update from a
                                        // remote provider to one of our processes
                                        return Ok(());
                                    }
                                }
                                // failed to send subscription update to process,
                                // unsubscribe from provider and close
                                verbose_print(
                                    &state.print_tx,
                                    "eth: got eth_sub_result but provider node did not match or local sub was already closed",
                                )
                                .await;
                                sub.close(sub_id, state).await;
                                sub_map.remove(&sub_id);
                                return Ok(());
                            }
                        }
                    }
                    // tell the remote provider that we don't have this sub
                    // so they can stop sending us updates
                    verbose_print(
                        &state.print_tx,
                        &format!(
                            "eth: got eth_sub_result but no matching sub {} found, unsubscribing",
                            sub_id
                        ),
                    )
                    .await;
                    kernel_message(
                        &state.our,
                        km.id,
                        km.source,
                        None,
                        true,
                        None,
                        EthAction::UnsubscribeLogs(sub_id),
                        &state.send_to_loop,
                    )
                    .await;
                    Ok(())
                }
                IncomingReq::SubKeepalive(sub_id) => {
                    // source expects that we have a local sub for them with this id
                    // if we do, no action required, otherwise, throw them an error.
                    if let Some(sub_map) = state.active_subscriptions.get(&km.source) {
                        if sub_map.contains_key(&sub_id) {
                            return Ok(());
                        } else if sub_map.is_empty() {
                            drop(sub_map);
                            state.active_subscriptions.remove(&km.source);
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
                    Ok(())
                }
            }
        }
    }
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
            "eth: handling {} from {}; active_subs len: {:?}",
            //"eth: handling {} from {}",
            match &eth_action {
                EthAction::SubscribeLogs { .. } => "subscribe",
                EthAction::UnsubscribeLogs(_) => "unsubscribe",
                EthAction::Request { .. } => "request",
            },
            km.source,
            state
                .active_subscriptions
                .iter()
                .map(|v| v.len())
                .collect::<Vec<_>>(),
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
            let Some(mut sub_map) = state.active_subscriptions.get_mut(&km.source) else {
                verbose_print(
                    &state.print_tx,
                    &format!(
                        "eth: got unsubscribe from {} but no subscription found",
                        km.source
                    ),
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
                return Ok(());
            };
            if let Some(sub) = sub_map.remove(&sub_id) {
                sub.close(sub_id, state).await;
                verbose_print(
                    &state.print_tx,
                    &format!("eth: closed subscription {} for {}", sub_id, km.source.node),
                )
                .await;
                kernel_message(
                    &state.our,
                    km.id,
                    km.rsvp.unwrap_or(km.source.clone()),
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
                    &format!(
                        "eth: got unsubscribe from {} but no subscription {} found",
                        km.source, sub_id
                    ),
                )
                .await;
                error_message(
                    &state.our,
                    km.id,
                    km.source.clone(),
                    EthError::MalformedRequest,
                    &state.send_to_loop,
                )
                .await;
            }
            // if sub_map is now empty, remove the source from the active_subscriptions map
            if sub_map.is_empty() {
                drop(sub_map);
                state.active_subscriptions.remove(&km.source);
            }
        }
        EthAction::Request { .. } => {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            state.response_channels.insert(km.id, sender);
            let our = state.our.to_string();
            let send_to_loop = state.send_to_loop.clone();
            let providers = state.providers.clone();
            let response_channels = state.response_channels.clone();
            let print_tx = state.print_tx.clone();
            let mut request_cache = Arc::clone(&state.request_cache);
            tokio::spawn(async move {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(timeout),
                    fulfill_request(
                        &our,
                        km.id,
                        &send_to_loop,
                        &eth_action,
                        &providers,
                        &mut receiver,
                        &print_tx,
                        &mut request_cache,
                    ),
                )
                .await
                {
                    Ok(response) => {
                        if let EthResponse::Err(EthError::RpcError(_)) = response {
                            // try one more time after 1s delay in case RPC is rate limiting
                            std::thread::sleep(std::time::Duration::from_millis(DELAY_MS));
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(timeout),
                                fulfill_request(
                                    &our,
                                    km.id,
                                    &send_to_loop,
                                    &eth_action,
                                    &providers,
                                    &mut receiver,
                                    &print_tx,
                                    &mut request_cache,
                                ),
                            )
                            .await
                            {
                                Ok(response) => {
                                    kernel_message(
                                        &our,
                                        km.id,
                                        km.rsvp.clone().unwrap_or(km.source.clone()),
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
                                    error_message(
                                        &our,
                                        km.id,
                                        km.source.clone(),
                                        EthError::RpcTimeout,
                                        &send_to_loop,
                                    )
                                    .await;
                                }
                            }
                        } else {
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
    eth_action: &EthAction,
    providers: &Providers,
    remote_request_receiver: &mut ProcessMessageReceiver,
    print_tx: &PrintSender,
    request_cache: &mut RequestCache,
) -> EthResponse {
    let serialized_action = serde_json::to_vec(eth_action).unwrap();
    let EthAction::Request {
        ref chain_id,
        ref method,
        ref params,
    } = eth_action
    else {
        return EthResponse::Err(EthError::PermissionDenied); // will never hit
    };
    {
        let mut request_cache = request_cache.lock().await;
        if let Some((cache_hit, time_of_hit)) = request_cache.shift_remove(&serialized_action) {
            // refresh cache entry (it is most recently accessed) & return it
            if time_of_hit.elapsed() < Duration::from_millis(DELAY_MS) {
                request_cache.insert(serialized_action, (cache_hit.clone(), time_of_hit));
                return cache_hit;
            }
        }
    }
    let Some(method) = valid_method(&method) else {
        return EthResponse::Err(EthError::InvalidMethod(method.to_string()));
    };
    let urls = {
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
    for mut url_provider in urls.into_iter() {
        let (pubsub, newly_activated) = match url_provider.pubsub.first() {
            Some(pubsub) => (pubsub, false),
            None => {
                if let Ok(()) = activate_url_provider(&mut url_provider).await {
                    verbose_print(
                        print_tx,
                        &format!("eth: activated url provider {}", url_provider.url),
                    )
                    .await;
                    (url_provider.pubsub.last().unwrap(), true)
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
        match pubsub.raw_request(method.into(), params).await {
            Ok(value) => {
                let mut is_replacement_successful = true;
                providers.entry(chain_id.clone()).and_modify(|aps| {
                    let Some(index) = find_index(
                        &aps.urls.iter().map(|u| u.url.as_str()).collect(),
                        &url_provider.url,
                    ) else {
                        is_replacement_successful = false;
                        return ();
                    };
                    let mut old_provider = aps.urls.remove(index);
                    if newly_activated {
                        old_provider.pubsub.push(url_provider.pubsub.pop().unwrap());
                    }
                    aps.urls.insert(0, old_provider);
                });
                if !is_replacement_successful {
                    verbose_print(
                        print_tx,
                        &format!("eth: unexpectedly couldn't find provider to be modified"),
                    )
                    .await;
                }
                let response = EthResponse::Response(value);
                let mut request_cache = request_cache.lock().await;
                if request_cache.len() >= MAX_REQUEST_CACHE_LEN {
                    // drop 10% oldest cache entries
                    request_cache.drain(0..MAX_REQUEST_CACHE_LEN / 10);
                }
                request_cache.insert(serialized_action, (response.clone(), Instant::now()));
                return response;
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
                    let err_value =
                        serde_json::to_value(err).unwrap_or_else(|_| serde_json::Value::Null);
                    return EthResponse::Err(EthError::RpcError(err_value));
                }
                if !newly_activated {
                    // this provider failed and needs to be reset
                    let mut is_reset_successful = true;
                    providers.entry(chain_id.clone()).and_modify(|aps| {
                        let Some(index) = find_index(
                            &aps.urls.iter().map(|u| u.url.as_str()).collect(),
                            &url_provider.url,
                        ) else {
                            is_reset_successful = false;
                            return ();
                        };
                        let mut url = aps.urls.remove(index);
                        url.pubsub = vec![];
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
                node_provider.hns_update.name
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
            remote_request_receiver,
        )
        .await;
        if let EthResponse::Err(e) = response {
            if let EthError::RpcMalformedResponse = e {
                set_node_unusable(
                    &providers,
                    &chain_id,
                    &node_provider.hns_update.name,
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
    if !node_provider.usable || node_provider.hns_update.name == our {
        return EthResponse::Err(EthError::PermissionDenied);
    }
    kernel_message(
        our,
        km_id,
        Address {
            node: node_provider.hns_update.name.clone(),
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
            for entry in new_providers.0.into_iter().rev() {
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
            state.home_directory_path.join(".eth_access_settings"),
            serde_json::to_string(&state.access_settings).unwrap(),
        )
        .await
        {
            verbose_print(&state.print_tx, "eth: saved new access settings").await;
        };
    }
    if save_providers {
        if let Ok(()) = tokio::fs::write(
            state.home_directory_path.join(".eth_providers"),
            serde_json::to_string(&providers_to_saved_configs(&state.providers)).unwrap(),
        )
        .await
        {
            verbose_print(&state.print_tx, "eth: saved new provider settings").await;
        };
    }
    EthConfigResponse::Ok
}
