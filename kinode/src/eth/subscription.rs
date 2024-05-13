use crate::eth::*;
use alloy_pubsub::RawSubscription;
use alloy_rpc_types::pubsub::SubscriptionResult;

/// cleans itself up when the subscription is closed or fails.
pub async fn create_new_subscription(
    state: &ModuleState,
    km_id: u64,
    target: Address,
    rsvp: Option<Address>,
    sub_id: u64,
    eth_action: EthAction,
) {
    let our = state.our.clone();
    let send_to_loop = state.send_to_loop.clone();
    let active_subscriptions = state.active_subscriptions.clone();
    let providers = state.providers.clone();
    let response_channels = state.response_channels.clone();
    let print_tx = state.print_tx.clone();
    tokio::spawn(async move {
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            build_subscription(
                &our,
                km_id,
                &target,
                &send_to_loop,
                &eth_action,
                &providers,
                &response_channels,
                &print_tx,
            ),
        )
        .await
        {
            // if building the subscription fails, send an error message to the target
            Ok(Err(e)) => {
                error_message(&our, km_id, target.clone(), e, &send_to_loop).await;
            }
            // if building the subscription times out, send an error message to the target
            Err(_) => {
                error_message(
                    &our,
                    km_id,
                    target.clone(),
                    EthError::RpcTimeout,
                    &send_to_loop,
                )
                .await;
            }
            // if building the subscription is successful, start the subscription
            // and in this task, maintain and clean up after it.
            Ok(Ok(maybe_raw_sub)) => {
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
                let our = our.clone();
                let send_to_loop = send_to_loop.clone();
                let print_tx = print_tx.clone();
                let active_subscriptions = active_subscriptions.clone();
                match maybe_raw_sub {
                    Ok(rx) => {
                        subs.insert(
                            sub_id,
                            // this is a local sub, as in, we connect to the rpc endpoint
                            ActiveSub::Local(tokio::spawn(async move {
                                // await the subscription error and kill it if so
                                let e = maintain_local_subscription(
                                    &our,
                                    sub_id,
                                    rx,
                                    &target,
                                    &rsvp,
                                    &send_to_loop,
                                    &active_subscriptions,
                                )
                                .await;
                                verbose_print(
                                    &print_tx,
                                    &format!("eth: closed local subscription due to error {e:?}"),
                                )
                                .await;
                                kernel_message(
                                    &our,
                                    rand::random(),
                                    target.clone(),
                                    rsvp,
                                    true,
                                    None,
                                    EthSubResult::Err(e),
                                    &send_to_loop,
                                )
                                .await;
                            })),
                        );
                    }
                    Err((provider_node, remote_sub_id)) => {
                        // this is a remote sub, given by a relay node
                        let (sender, rx) = tokio::sync::mpsc::channel(10);
                        let keepalive_km_id = rand::random();
                        let (keepalive_err_sender, keepalive_err_receiver) =
                            tokio::sync::mpsc::channel(1);
                        response_channels.insert(keepalive_km_id, keepalive_err_sender);
                        let response_channels = response_channels.clone();
                        subs.insert(
                            remote_sub_id,
                            ActiveSub::Remote {
                                provider_node: provider_node.clone(),
                                handle: tokio::spawn(async move {
                                    let e = maintain_remote_subscription(
                                        &our,
                                        &provider_node,
                                        remote_sub_id,
                                        sub_id,
                                        keepalive_km_id,
                                        rx,
                                        keepalive_err_receiver,
                                        &target,
                                        &send_to_loop,
                                        &active_subscriptions,
                                        &response_channels,
                                    )
                                    .await;
                                    verbose_print(
                                        &print_tx,
                                        &format!("eth: closed subscription with provider node due to error {e:?}"),
                                    )
                                    .await;
                                    kernel_message(
                                        &our,
                                        rand::random(),
                                        target.clone(),
                                        None,
                                        true,
                                        None,
                                        EthSubResult::Err(e),
                                        &send_to_loop,
                                    )
                                    .await;
                                }),
                                sender,
                            },
                        );
                    }
                }
            }
        }
    });
}

/// terrible abuse of result in return type, yes, sorry
async fn build_subscription(
    our: &str,
    km_id: u64,
    target: &Address,
    send_to_loop: &MessageSender,
    eth_action: &EthAction,
    providers: &Providers,
    response_channels: &ResponseChannels,
    print_tx: &PrintSender,
) -> Result<Result<RawSubscription, (String, u64)>, EthError> {
    let EthAction::SubscribeLogs {
        chain_id,
        kind,
        params,
        ..
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

    // bump the successful provider to the front of the list for future requests
    for (index, url_provider) in aps.urls.iter_mut().enumerate() {
        let pubsub = match &url_provider.pubsub {
            Some(pubsub) => pubsub,
            None => {
                if let Ok(()) = activate_url_provider(url_provider).await {
                    verbose_print(
                        &print_tx,
                        &format!("eth: activated url provider {}", url_provider.url),
                    )
                    .await;
                    url_provider.pubsub.as_ref().unwrap()
                } else {
                    verbose_print(
                        &print_tx,
                        &format!("eth: could not activate url provider {}", url_provider.url),
                    )
                    .await;
                    continue;
                }
            }
        };
        let kind = serde_json::to_value(&kind).unwrap();
        let params = serde_json::to_value(&params).unwrap();
        match pubsub
            .inner()
            .prepare("eth_subscribe", [kind, params])
            .await
        {
            Ok(id) => {
                let rx = pubsub.inner().get_raw_subscription(id).await;
                let successful_provider = aps.urls.remove(index);
                aps.urls.insert(0, successful_provider);
                return Ok(Ok(rx));
            }
            Err(rpc_error) => {
                verbose_print(
                    &print_tx,
                    &format!(
                        "eth: got error from url provider {}: {}",
                        url_provider.url, rpc_error
                    ),
                )
                .await;
                // this provider failed and needs to be reset
                url_provider.pubsub = None;
            }
        }
    }

    let (sender, mut response_receiver) = tokio::sync::mpsc::channel(1);
    response_channels.insert(km_id, sender);
    // we need to create our own unique sub id because in the remote provider node,
    // all subs will be identified under our process address.
    let remote_sub_id = rand::random();
    for node_provider in &mut aps.nodes {
        verbose_print(
            &print_tx,
            &format!(
                "eth: attempting to fulfill via {}",
                node_provider.kns_update.name
            ),
        )
        .await;
        match forward_to_node_provider(
            &our,
            km_id,
            Some(target.clone()),
            node_provider,
            EthAction::SubscribeLogs {
                sub_id: remote_sub_id,
                chain_id: chain_id.clone(),
                kind: kind.clone(),
                params: params.clone(),
            },
            &send_to_loop,
            &mut response_receiver,
        )
        .await
        {
            EthResponse::Ok => {
                kernel_message(
                    &our,
                    km_id,
                    target.clone(),
                    None,
                    false,
                    None,
                    EthResponse::Ok,
                    &send_to_loop,
                )
                .await;
                response_channels.remove(&km_id);
                return Ok(Err((node_provider.kns_update.name.clone(), remote_sub_id)));
            }
            EthResponse::Response { .. } => {
                // the response to a SubscribeLogs request must be an 'ok'
                node_provider.usable = false;
            }
            EthResponse::Err(e) => {
                if e == EthError::RpcMalformedResponse {
                    node_provider.usable = false;
                }
            }
        }
    }
    response_channels.remove(&km_id);
    return Err(EthError::NoRpcForChain);
}

async fn maintain_local_subscription(
    our: &str,
    sub_id: u64,
    mut rx: RawSubscription,
    target: &Address,
    rsvp: &Option<Address>,
    send_to_loop: &MessageSender,
    active_subscriptions: &ActiveSubscriptions,
) -> EthSubError {
    while let Ok(value) = rx.recv().await {
        let result: SubscriptionResult = match serde_json::from_str(value.get()) {
            Ok(res) => res,
            Err(e) => {
                return EthSubError {
                    id: sub_id,
                    error: e.to_string(),
                }
            }
        };
        kernel_message(
            our,
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
    active_subscriptions
        .entry(target.clone())
        .and_modify(|sub_map| {
            sub_map.remove(&sub_id);
        });
    EthSubError {
        id: sub_id,
        error: "subscription closed unexpectedly".to_string(),
    }
}

/// handle the subscription updates from a remote provider,
/// and also perform keepalive checks on that provider.
/// current keepalive is 30s, this can be adjusted as desired
///
/// if the subscription goes more than 2 hours without an update,
/// the provider will be considered dead and the subscription will be closed.
async fn maintain_remote_subscription(
    our: &str,
    provider_node: &str,
    remote_sub_id: u64,
    sub_id: u64,
    keepalive_km_id: u64,
    mut rx: tokio::sync::mpsc::Receiver<EthSubResult>,
    mut net_error_rx: ProcessMessageReceiver,
    target: &Address,
    send_to_loop: &MessageSender,
    active_subscriptions: &ActiveSubscriptions,
    response_channels: &ResponseChannels,
) -> EthSubError {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    let mut last_received = tokio::time::Instant::now();
    let two_hours = tokio::time::Duration::from_secs(2 * 3600);

    let e = loop {
        tokio::select! {
            incoming = rx.recv() => {
                match incoming {
                    Some(EthSubResult::Ok(upd)) => {
                        // Update the last received time on any successful sub result
                        last_received = tokio::time::Instant::now();
                        kernel_message(
                            &our,
                            rand::random(),
                            target.clone(),
                            None,
                            true,
                            None,
                            EthSubResult::Ok(EthSub {
                                id: sub_id,
                                result: upd.result,
                            }),
                            &send_to_loop,
                        )
                        .await;
                    }
                    Some(EthSubResult::Err(e)) => {
                        break EthSubError {
                            id: sub_id,
                            error: e.error,
                        };
                    }
                    None => {
                        break EthSubError {
                            id: sub_id,
                            error: "subscription closed unexpectedly".to_string(),
                        };
                    }
                }
            }
            _ = interval.tick() => {
                // perform keepalive
                kernel_message(
                    &our,
                    keepalive_km_id,
                    Address { node: provider_node.to_string(), process: ETH_PROCESS_ID.clone() },
                    None,
                    true,
                    Some(30),
                    IncomingReq::SubKeepalive(remote_sub_id),
                    &send_to_loop,
                ).await;
            }
            _incoming = net_error_rx.recv() => {
                break EthSubError {
                    id: sub_id,
                    error: "subscription node-provider failed keepalive".to_string(),
                };
            }
            _ = tokio::time::sleep_until(last_received + two_hours) => {
                break EthSubError {
                    id: sub_id,
                    error: "No updates received for 2 hours, subscription considered dead.".to_string(),
                };
            }
        }
    };
    active_subscriptions
        .entry(target.clone())
        .and_modify(|sub_map| {
            sub_map.remove(&sub_id);
        });
    response_channels.remove(&keepalive_km_id);
    e
}
