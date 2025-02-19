use lib::{
    core::Address,
    types::core::{
        Identity, KernelMessage, MessageReceiver, MessageSender, NetAction, NetResponse,
        NetworkErrorSender, NodeRouting, PrintSender, NET_PROCESS_ID,
    },
};
use types::{
    ActivePassthroughs, IdentityExt, NetData, OnchainPKI, Peers, PendingPassthroughs, TCP_PROTOCOL,
    WS_PROTOCOL,
};
use {dashmap::DashMap, ring::signature::Ed25519KeyPair, std::sync::Arc, tokio::task::JoinSet};

mod connect;
mod indirect;
mod tcp;
mod types;
mod utils;
mod ws;

/// Entry point for all node to node networking. Manages the "working version" of the PKI,
/// which may not be the complete PKI. Stateless: does not persist PKI information, only
/// ingests it from [`NetAction::HnsUpdate`] and [`NetAction::HnsBatchUpdate`] requests.
///
/// Handles messages from kernel that are directed at other nodes by locating that node
/// in the PKI and finding a usable route to them, if any. Nodes can present indirect
/// or direct networking in the PKI. If direct, it can be over a number of protocols.
/// This implementation supports two: `"ws"` and `"tcp"`. These are keys associated
/// with ports in the `ports` field of a node [`Identity`].
pub async fn networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    kernel_message_rx: MessageReceiver,
    // only used if indirect -- TODO use
    _reveal_ip: bool,
    max_peers: u64,
    // only used by routers
    max_passthroughs: u64,
) -> anyhow::Result<()> {
    crate::fd_manager::send_fd_manager_request_fds_limit(
        &Address::new(&our.name, NET_PROCESS_ID.clone()),
        &kernel_message_tx,
    )
    .await;

    let ext = IdentityExt {
        our: Arc::new(our),
        our_ip: Arc::new(our_ip),
        keypair,
        kernel_message_tx,
        network_error_tx,
        print_tx,
        _reveal_ip,
    };
    // start by initializing the structs where we'll store PKI in memory
    // and store a mapping of peers we have an active route for
    let pki: OnchainPKI = Arc::new(DashMap::new());
    let peers: Peers = Peers::new(max_peers, ext.kernel_message_tx.clone());
    // only used by routers
    let pending_passthroughs: PendingPassthroughs = Arc::new(DashMap::new());
    let active_passthroughs: ActivePassthroughs = Arc::new(DashMap::new());

    let net_data = NetData {
        pki,
        peers,
        pending_passthroughs,
        active_passthroughs,
        max_passthroughs,
        fds_limit: 10, // small hardcoded limit that gets replaced by fd-manager soon after boot
    };

    let mut tasks = JoinSet::<anyhow::Result<()>>::new();

    // spawn the task for handling messages from the kernel,
    // and depending on the ports in our identity, the tasks
    // for ws and/or tcp, or indirect routing.
    tasks.spawn(local_recv(ext.clone(), kernel_message_rx, net_data.clone()));

    match &ext.our.routing {
        NodeRouting::Direct { ip, ports } => {
            if *ext.our_ip != *ip {
                return Err(anyhow::anyhow!(
                    "net: fatal error: IP address mismatch: {} != {}, update your HNS identity",
                    ext.our_ip,
                    ip
                ));
            }
            utils::print_debug(&ext.print_tx, "going online as a direct node").await;
            if !ports.contains_key(WS_PROTOCOL) && !ports.contains_key(TCP_PROTOCOL) {
                return Err(anyhow::anyhow!(
                    "net: fatal error: need at least one networking protocol"
                ));
            }
            if ext.our.ws_routing().is_some() {
                tasks.spawn(ws::receiver(ext.clone(), net_data.clone()));
            }
            if ext.our.tcp_routing().is_some() {
                tasks.spawn(tcp::receiver(ext.clone(), net_data.clone()));
            }
        }
        NodeRouting::Routers(routers) | NodeRouting::Both { routers, .. } => {
            if routers.is_empty() {
                return Err(anyhow::anyhow!(
                    "net: fatal error: need at least one router, update your HNS identity"
                ));
            }
            utils::print_debug(&ext.print_tx, "going online as an indirect node").await;
            // if we are indirect, we need to establish a route to each router
            // and then listen for incoming connections on each of them.
            // this task will periodically check and re-connect to routers
            tasks.spawn(indirect::maintain_routers(ext.clone(), net_data.clone()));
        }
    }

    // if any of these tasks complete, we should exit with an error
    tasks.join_next().await.unwrap()?
}

/// handle messages from the kernel. if the `target` is our node-id, we handle
/// it. otherwise, we treat it as a message to be routed.
async fn local_recv(
    ext: IdentityExt,
    mut kernel_message_rx: MessageReceiver,
    mut data: NetData,
) -> anyhow::Result<()> {
    while let Some(km) = kernel_message_rx.recv().await {
        if km.target.node == ext.our.name {
            // handle messages sent to us
            handle_message(&ext, km, &mut data).await;
        } else {
            connect::send_to_peer(&ext, &data, km).await;
        }
    }
    Err(anyhow::anyhow!("net: kernel message channel was dropped"))
}

async fn handle_message(ext: &IdentityExt, km: KernelMessage, data: &mut NetData) {
    match &km.message {
        lib::core::Message::Request(request) => handle_request(ext, &km, &request.body, data).await,
        lib::core::Message::Response((response, _context)) => {
            handle_response(&km, &response.body, data).await
        }
    }
}

async fn handle_request(
    ext: &IdentityExt,
    km: &KernelMessage,
    request_body: &[u8],
    data: &mut NetData,
) {
    if km.source.node == ext.our.name {
        handle_local_request(ext, km, request_body, data).await;
    } else {
        match handle_remote_request(ext, km, request_body, data).await {
            Ok(()) => return,
            Err(e) => utils::print_debug(&ext.print_tx, &e.to_string()).await,
        }
    }
}

async fn handle_local_request(
    ext: &IdentityExt,
    km: &KernelMessage,
    request_body: &[u8],
    data: &mut NetData,
) {
    match rmp_serde::from_slice::<NetAction>(request_body) {
        Err(_e) => {
            // only other possible message is from fd-manager -- handle here
            handle_fdman(km, request_body, data).await;
        }
        Ok(NetAction::ConnectionRequest(_)) => {
            // we shouldn't get these locally, ignore
        }
        Ok(NetAction::HnsUpdate(log)) => {
            utils::ingest_log(log, &data.pki);
        }
        Ok(NetAction::HnsBatchUpdate(logs)) => {
            for log in logs {
                utils::ingest_log(log, &data.pki);
            }
        }
        Ok(gets) => {
            let (response_body, response_blob) = match gets {
                NetAction::GetPeers => (
                    NetResponse::Peers(
                        data.peers
                            .peers()
                            .iter()
                            .map(|p| p.identity.clone())
                            .collect::<Vec<Identity>>(),
                    ),
                    None,
                ),
                NetAction::GetPeer(peer) => (
                    if peer == ext.our.name {
                        NetResponse::Peer(Some((*ext.our).clone()))
                    } else {
                        NetResponse::Peer(data.pki.get(&peer).map(|p| p.clone()))
                    },
                    None,
                ),
                NetAction::GetDiagnostics => {
                    let mut printout = String::new();
                    printout.push_str(&format!(
                        "indexing from contract address {}\r\n",
                        crate::HYPERMAP_ADDRESS
                    ));
                    printout.push_str(&format!("our Identity: {:#?}\r\n", ext.our));
                    printout.push_str(&format!(
                        "we have connections with {} peers ({} max):\r\n",
                        data.peers.peers().len(),
                        data.peers.max_peers(),
                    ));

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    for peer in data.peers.peers().iter() {
                        printout.push_str(&format!(
                            "    {},{} last message {}s ago\r\n",
                            peer.identity.name,
                            if peer.routing_for { " (routing)" } else { "" },
                            now.saturating_sub(peer.last_message)
                        ));
                    }

                    if data.max_passthroughs > 0 {
                        printout.push_str(&format!(
                            "we allow {} max passthroughs\r\n",
                            data.max_passthroughs
                        ));
                    }

                    if !data.pending_passthroughs.is_empty() {
                        printout.push_str(&format!(
                            "we have {} pending passthroughs:\r\n",
                            data.pending_passthroughs.len()
                        ));
                        for p in data.pending_passthroughs.iter() {
                            printout.push_str(&format!("    {} -> {}\r\n", p.key().0, p.key().1));
                        }
                    }

                    if !data.active_passthroughs.is_empty() {
                        printout.push_str(&format!(
                            "we have {} active passthroughs:\r\n",
                            data.active_passthroughs.len()
                        ));
                        for p in data.active_passthroughs.iter() {
                            printout.push_str(&format!("    {} -> {}\r\n", p.key().0, p.key().1));
                        }
                    }

                    printout.push_str(&format!(
                        "we have {} entries in the PKI\r\n",
                        data.pki.len()
                    ));

                    (NetResponse::Diagnostics(printout), None)
                }
                NetAction::Sign => (
                    NetResponse::Signed,
                    Some(lib::core::LazyLoadBlob {
                        mime: None,
                        bytes: ext
                            .keypair
                            .sign(
                                &[
                                    km.source.to_string().as_bytes(),
                                    &km.lazy_load_blob
                                        .as_ref()
                                        .unwrap_or(&lib::core::LazyLoadBlob {
                                            mime: None,
                                            bytes: vec![],
                                        })
                                        .bytes,
                                ]
                                .concat(),
                            )
                            .as_ref()
                            .to_vec(),
                    }),
                ),
                NetAction::Verify { from, signature } => {
                    let message = [
                        from.to_string().as_bytes(),
                        &km.lazy_load_blob
                            .as_ref()
                            .unwrap_or(&lib::core::LazyLoadBlob {
                                mime: None,
                                bytes: vec![],
                            })
                            .bytes,
                    ]
                    .concat();
                    (
                        NetResponse::Verified(utils::validate_signature(
                            &from.node, &signature, &message, &data.pki,
                        )),
                        None,
                    )
                }
                _ => {
                    // already matched these outcomes
                    return;
                }
            };
            KernelMessage::builder()
                .id(km.id)
                .source((ext.our.name.as_str(), "net", "distro", "sys"))
                .target(km.rsvp.as_ref().unwrap_or(&km.source).clone())
                .message(lib::core::Message::Response((
                    lib::core::Response {
                        inherit: false,
                        body: rmp_serde::to_vec(&response_body)
                            .expect("net: failed to serialize response"),
                        metadata: None,
                        capabilities: vec![],
                    },
                    None,
                )))
                .lazy_load_blob(response_blob)
                .build()
                .unwrap()
                .send(&ext.kernel_message_tx)
                .await;
        }
    }
}

async fn handle_fdman(km: &KernelMessage, request_body: &[u8], data: &mut NetData) {
    if km.source.process != *lib::core::FD_MANAGER_PROCESS_ID {
        return;
    }
    let Ok(req) = serde_json::from_slice::<lib::core::FdManagerRequest>(request_body) else {
        return;
    };
    match req {
        lib::core::FdManagerRequest::FdsLimit(fds_limit) => {
            data.fds_limit = fds_limit;
            data.peers.set_max_peers(fds_limit);
            // TODO combine with max_peers check
            // only update passthrough limit if it's higher than the new fds limit
            // most nodes have passthroughs disabled, meaning this will keep it at 0
            if data.max_passthroughs > fds_limit {
                data.max_passthroughs = fds_limit;
            }
            // TODO cull passthroughs too
            if data.peers.peers().len() >= data.fds_limit as usize {
                let diff = data.peers.peers().len() - data.fds_limit as usize;
                println!("net: culling {diff} peer(s)\r\n");
                data.peers.cull(diff).await;
            }
        }
        _ => return,
    }
}

async fn handle_remote_request(
    ext: &IdentityExt,
    km: &KernelMessage,
    request_body: &[u8],
    data: &NetData,
) -> anyhow::Result<()> {
    match rmp_serde::from_slice::<NetAction>(request_body) {
        Ok(NetAction::HnsBatchUpdate(_)) | Ok(NetAction::HnsUpdate(_)) => {
            // for now, we don't get these from remote, only locally.
            return Err(anyhow::anyhow!(
                "net: not allowed to update PKI from remote"
            ));
        }
        Ok(NetAction::ConnectionRequest(from)) => {
            // someone wants to open a passthrough with us through a router.
            // if we are an indirect node, and source is one of our routers,
            // respond by attempting to init a matching passthrough.
            let allowed_routers = match &ext.our.routing {
                NodeRouting::Routers(routers) => routers,
                _ => return Err(anyhow::anyhow!("net: not an indirect node")),
            };
            if !allowed_routers.contains(&km.source.node) {
                return Err(anyhow::anyhow!("net: not one of our routers"));
            }
            let Some(router_id) = data.pki.get(&km.source.node) else {
                return Err(anyhow::anyhow!("net: router not in PKI"));
            };
            let Some(peer_id) = data.pki.get(&from) else {
                return Err(anyhow::anyhow!("net: peer not in PKI"));
            };
            // pick a protocol to connect to router with
            // spawn a task that has a timeout here to not block the loop
            let ext = ext.clone();
            let data = data.clone();
            let peer_id = peer_id.clone();
            let router_id = router_id.clone();
            tokio::spawn(tokio::time::timeout(
                std::time::Duration::from_secs(5),
                async move {
                    if router_id.tcp_routing().is_some() {
                        tcp::recv_via_router(ext, data, peer_id, router_id).await;
                    } else if router_id.ws_routing().is_some() {
                        ws::recv_via_router(ext, data, peer_id, router_id).await;
                    }
                },
            ));
        }
        _ => {
            // if we can't parse this to a NetAction, treat it as a hello and print it,
            // and respond with a simple "ack" response
            utils::parse_hello_message(
                &ext.our,
                &km,
                request_body,
                &ext.kernel_message_tx,
                &ext.print_tx,
            )
            .await;
        }
    }
    Ok(())
}

// Responses are received as a router, when we send ConnectionRequests
// to a node we do routing for.
async fn handle_response(km: &KernelMessage, response_body: &[u8], data: &NetData) {
    match rmp_serde::from_slice::<lib::core::NetResponse>(response_body) {
        Ok(lib::core::NetResponse::Rejected(to)) => {
            // drop from our pending map
            // this will drop the socket, causing initiator to see it as failed
            data.pending_passthroughs
                .remove(&(to, km.source.node.to_owned()));
        }
        _ => {
            // ignore any other response, for now
        }
    }
}
