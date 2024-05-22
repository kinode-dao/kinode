use lib::types::core::{
    Address, Identity, KernelMessage, MessageReceiver, MessageSender, NetAction, NetResponse,
    NetworkErrorSender, NodeRouting, PrintSender, ProcessId,
};
use types::{IdentityExt, NetData, OnchainPKI, PKINames, Peers};
use {dashmap::DashMap, ring::signature::Ed25519KeyPair, std::sync::Arc, tokio::task::JoinSet};

mod connect;
mod indirect;
mod tcp;
pub mod types;
mod utils;
mod ws;

/// Entry point for all node to node networking. Manages the "working version" of the PKI,
/// which may not be the complete PKI. Stateless: does not persist PKI information, only
/// ingests it from [`NetAction::KnsUpdate`] and [`NetAction::KnsBatchUpdate`] requests.
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
    self_message_tx: MessageSender,
    kernel_message_rx: MessageReceiver,
    reveal_ip: bool, // only used if indirect
) -> anyhow::Result<()> {
    let ext = IdentityExt {
        our: Arc::new(our),
        our_ip: Arc::new(our_ip),
        keypair,
        kernel_message_tx,
        network_error_tx,
        print_tx,
        self_message_tx,
        reveal_ip,
    };
    // start by initializing the structs where we'll store PKI in memory
    // and store a mapping of peers we have an active route for
    let pki: OnchainPKI = Arc::new(DashMap::new());
    let peers: Peers = Arc::new(DashMap::new());
    // keep a mapping of namehashes (used onchain) to node-ids.
    // this allows us to act as a translator for others, and translate
    // our own router namehashes if we are indirect.
    let names: PKINames = Arc::new(DashMap::new());

    let net_data = NetData { pki, peers, names };

    let mut tasks = JoinSet::<anyhow::Result<()>>::new();

    // spawn the task for handling messages from the kernel,
    // and depending on the ports in our identity, the tasks
    // for ws and/or tcp, or indirect routing.
    tasks.spawn(local_recv(ext.clone(), kernel_message_rx, net_data.clone()));

    match &ext.our.routing {
        NodeRouting::Direct { ip, ports } => {
            if *ext.our_ip != *ip {
                return Err(anyhow::anyhow!(
                    "net: fatal error: IP address mismatch: {} != {}, update your KNS identity",
                    ext.our_ip,
                    ip
                ));
            }
            utils::print_loud(&ext.print_tx, "going online as a direct node").await;
            if ports.contains_key("ws") {
                tasks.spawn(ws::receiver(ext.clone(), net_data.clone()));
            }
            if ports.contains_key("tcp") {
                tasks.spawn(tcp::receiver(ext.clone(), net_data.clone()));
            }
        }
        NodeRouting::Routers(routers) | NodeRouting::Both { routers, .. } => {
            if routers.is_empty() {
                return Err(anyhow::anyhow!(
                    "net: fatal error: need at least one router, update your KNS identity"
                ));
            }
            utils::print_loud(&ext.print_tx, "going online as an indirect node").await;
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
    data: NetData,
) -> anyhow::Result<()> {
    while let Some(km) = kernel_message_rx.recv().await {
        if km.target.node == ext.our.name {
            // handle messages sent to us
            handle_message(&ext, &km, &data).await;
        } else {
            connect::send_to_peer(&ext, &data, km).await;
        }
    }
    Err(anyhow::anyhow!("net: kernel message channel was dropped"))
}

async fn handle_message(ext: &IdentityExt, km: &KernelMessage, data: &NetData) {
    match &km.message {
        lib::core::Message::Request(request) => handle_request(ext, km, &request.body, data).await,
        lib::core::Message::Response((response, _context)) => {
            handle_response(ext, km, &response.body, data).await
        }
    }
}

async fn handle_request(
    ext: &IdentityExt,
    km: &KernelMessage,
    request_body: &[u8],
    data: &NetData,
) {
    if km.source.node == ext.our.name {
        handle_local_request(ext, km, request_body, data).await;
    } else {
        handle_remote_request(ext, km, request_body, data).await;
    }
}

async fn handle_local_request(
    ext: &IdentityExt,
    km: &KernelMessage,
    request_body: &[u8],
    data: &NetData,
) {
    match rmp_serde::from_slice::<NetAction>(request_body) {
        Err(_e) => {
            // ignore
        }
        Ok(NetAction::ConnectionRequest(_)) => {
            // we shouldn't get these locally, ignore
        }
        Ok(NetAction::KnsUpdate(log)) => {
            utils::ingest_log(log, &data.pki, &data.names);
        }
        Ok(NetAction::KnsBatchUpdate(logs)) => {
            for log in logs {
                utils::ingest_log(log, &data.pki, &data.names);
            }
        }
        Ok(gets) => {
            let (response_body, response_blob) = match gets {
                NetAction::GetPeers => (
                    NetResponse::Peers(
                        data.peers
                            .iter()
                            .map(|p| p.identity.clone())
                            .collect::<Vec<Identity>>(),
                    ),
                    None,
                ),
                NetAction::GetPeer(peer) => (
                    NetResponse::Peer(data.pki.get(&peer).map(|p| p.clone())),
                    None,
                ),
                NetAction::GetName(namehash) => (
                    NetResponse::Name(data.names.get(&namehash).map(|n| n.clone())),
                    None,
                ),
                NetAction::GetDiagnostics => {
                    let mut printout = String::new();
                    printout.push_str(&format!(
                        "indexing from contract address {}\r\n",
                        crate::KNS_ADDRESS
                    ));
                    printout.push_str(&format!("our Identity: {:#?}\r\n", ext.our));
                    printout.push_str("we have connections with peers:\r\n");
                    for peer in data.peers.iter() {
                        printout.push_str(&format!(
                            "    {}, routing_for={}\r\n",
                            peer.identity.name, peer.routing_for,
                        ));
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
            ext.kernel_message_tx
                .send(KernelMessage {
                    id: km.id,
                    source: Address {
                        node: ext.our.name.clone(),
                        process: ProcessId::new(Some("net"), "distro", "sys"),
                    },
                    target: km.rsvp.as_ref().unwrap_or(&km.source).clone(),
                    rsvp: None,
                    message: lib::core::Message::Response((
                        lib::core::Response {
                            inherit: false,
                            body: rmp_serde::to_vec(&response_body)
                                .expect("net: failed to serialize response"),
                            metadata: None,
                            capabilities: vec![],
                        },
                        None,
                    )),
                    lazy_load_blob: response_blob,
                })
                .await
                .expect("net: kernel channel was dropped");
        }
    }
}

async fn handle_remote_request(
    ext: &IdentityExt,
    km: &KernelMessage,
    request_body: &[u8],
    data: &NetData,
) {
    match rmp_serde::from_slice::<NetAction>(request_body) {
        Ok(NetAction::KnsBatchUpdate(_)) | Ok(NetAction::KnsUpdate(_)) => {
            // for now, we don't get these from remote, only locally.
        }
        Ok(NetAction::ConnectionRequest(from)) => {
            // someone wants to open a passthrough with us through a router.
            // if we are an indirect node, and source is one of our routers,
            // respond by attempting to init a matching passthrough.
            let allowed_routers = match ext.our.routing {
                NodeRouting::Routers(ref routers) => routers,
                _ => return,
            };
            if allowed_routers.contains(&km.source.node) {
                // connect back to that router and open a passthrough via them
                todo!();
            }
        }
        _ => {
            // if we can't parse this to a NetAction, treat it as a hello and print it,
            // and respond with a simple "delivered" response
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
}

// Responses are received as a router, when we send ConnectionRequests
// to a node we do routing for.
async fn handle_response(
    ext: &IdentityExt,
    km: &KernelMessage,
    response_body: &[u8],
    data: &NetData,
) {
    match rmp_serde::from_slice::<lib::core::NetResponse>(response_body) {
        Ok(lib::core::NetResponse::Accepted(_)) => {
            // TODO anything here?
        }
        Ok(lib::core::NetResponse::Rejected(to)) => {
            // drop from our pending map
            // this will drop the socket, causing initiator to see it as failed
            todo!();
            // pending_passthroughs
            //     .ok_or(anyhow!("got net response as non-router"))?
            //     .remove(&(to, km.source.node));
        }
        _ => {
            // ignore
        }
    }
}
