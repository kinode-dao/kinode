#[cfg(not(feature = "simulation-mode"))]
use {
    anyhow::{anyhow, Result},
    dashmap::DashMap,
    futures::{SinkExt, StreamExt},
    rand::seq::SliceRandom,
    ring::signature::Ed25519KeyPair,
    std::{collections::HashMap, sync::Arc},
    tokio::net::TcpListener,
    tokio::task::JoinSet,
    tokio::time,
    tokio_tungstenite::{
        accept_async, connect_async, tungstenite, MaybeTlsStream, WebSocketStream,
    },
};

#[cfg(not(feature = "simulation-mode"))]
mod types;
#[cfg(not(feature = "simulation-mode"))]
mod utils;
#[cfg(not(feature = "simulation-mode"))]
use crate::net::{types::*, utils::*};
#[cfg(not(feature = "simulation-mode"))]
use crate::types::*;

// Re-export for testing.
#[cfg(feature = "simulation-mode")]
mod mock;
#[cfg(feature = "simulation-mode")]
pub use mock::mock_client;

// only used in connection initialization, otherwise, nacks and Responses are only used for "timeouts"
#[cfg(not(feature = "simulation-mode"))]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 10 MB -- TODO analyze as desired, apps can always chunk data into many messages
/// note that this only applies to cross-network messages, not local ones.
#[cfg(not(feature = "simulation-mode"))]
const MESSAGE_MAX_SIZE: u32 = 10_485_800;

/// Entry point from the main kernel task. Runs forever, spawns listener and sender tasks.
#[cfg(not(feature = "simulation-mode"))]
pub async fn networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    message_rx: MessageReceiver,
    contract_address: String,
    reveal_ip: bool,
) -> Result<()> {
    // branch on whether we are a direct or indirect node
    match &our.ws_routing {
        None => {
            // indirect node: run the indirect networking strategy
            print_tx
                .send(Printout {
                    verbosity: 0,
                    content: "going online as an indirect node".to_string(),
                })
                .await?;
            indirect_networking(
                our,
                our_ip,
                keypair,
                kernel_message_tx,
                network_error_tx,
                print_tx,
                self_message_tx,
                message_rx,
                reveal_ip,
                contract_address,
            )
            .await
        }
        Some((ip, port)) => {
            // direct node: run the direct networking strategy
            if &our_ip != ip {
                return Err(anyhow!(
                    "net: fatal error: IP address mismatch: {} != {}, update your KNS identity",
                    our_ip,
                    ip
                ));
            }
            let tcp = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
                Ok(tcp) => tcp,
                Err(_e) => {
                    return Err(anyhow!(
                        "net: fatal error: can't listen on port {}, update your KNS identity or free up that port",
                        port,
                    ));
                }
            };
            print_tx
                .send(Printout {
                    verbosity: 0,
                    content: "going online as a direct node".to_string(),
                })
                .await?;
            direct_networking(
                our,
                our_ip,
                tcp,
                keypair,
                kernel_message_tx,
                network_error_tx,
                print_tx,
                self_message_tx,
                message_rx,
                contract_address,
            )
            .await
        }
    }
}

#[cfg(not(feature = "simulation-mode"))]
async fn indirect_networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    _self_message_tx: MessageSender,
    mut message_rx: MessageReceiver,
    reveal_ip: bool,
    contract_address: String,
) -> Result<()> {
    print_debug(&print_tx, "net: starting as indirect").await;
    let pki: OnchainPKI = Arc::new(DashMap::new());
    let peers: Peers = Arc::new(DashMap::new());
    // mapping from KNS namehash to username
    let names: PKINames = Arc::new(DashMap::new());
    // track peers that we're already in the midst of establishing a connection with
    let mut pending_connections = JoinSet::<(NodeId, Result<()>)>::new();
    let mut peer_message_queues = HashMap::<NodeId, Vec<KernelMessage>>::new();

    // some initial delay as we wait for KNS data to be piped in from kns_indexer
    let mut router_reconnect_delay = std::time::Duration::from_secs(2);

    loop {
        tokio::select! {
            // 1. receive messages from kernel and send out over connections,
            // making new connections through our router-set as needed
            Some(km) = message_rx.recv() => {
                // got a message from kernel to send out over the network
                let target = &km.target.node;
                // if the message is for us, it's either a protocol-level "hello" message,
                // or a debugging command issued from our terminal. handle it here:
                if target == &our.name {
                    match handle_local_message(
                        &our,
                        &our_ip,
                        &keypair,
                        km,
                        peers.clone(),
                        pki.clone(),
                        None,
                        None,
                        names.clone(),
                        &kernel_message_tx,
                        &print_tx,
                        &contract_address,
                    )
                    .await {
                        Ok(()) => continue,
                        Err(e) => {
                            print_tx.send(Printout {
                                verbosity: 2,
                                content: format!("net: error handling local message: {e}")
                            }).await?;
                            continue
                        }
                    }
                }
                // if the message is for a peer we currently have a connection with,
                // try to send it to them
                else if let Some(peer) = peers.get_mut(target) {
                    peer.sender.send(km)?;
                    continue
                }
                // if we cannot send it to an existing peer-connection, need to spawn
                // a task that will attempt to establish such a connection.
                // if such a task already exists for that peer, we should queue the message
                // to be sent once that task completes. otherwise, it will duplicate connections.
                pending_connections.spawn(establish_new_peer_connection(
                    our.clone(),
                    our_ip.clone(),
                    keypair.clone(),
                    km,
                    pki.clone(),
                    names.clone(),
                    peers.clone(),
                    reveal_ip,
                    kernel_message_tx.clone(),
                    network_error_tx.clone(),
                    print_tx.clone(),
                ));
            }
            // 2. recover the result of a pending connection and flush any message
            // queue that's built up since it was spawned
            Some(Ok((peer_name, result))) = pending_connections.join_next() => {
                match result {
                    Ok(()) => {
                        // if we have a message queue for this peer, send it out
                        if let Some(queue) = peer_message_queues.remove(&peer_name) {
                            for km in queue {
                                peers.get_mut(&peer_name).unwrap().sender.send(km)?;
                            }
                        }
                    }
                    Err(_e) => {
                        // TODO decide if this is good behavior, but throw
                        // offline error for each message in this peer's queue
                        if let Some(queue) = peer_message_queues.remove(&peer_name) {
                            for km in queue {
                                error_offline(km, &network_error_tx).await?;
                            }
                        }
                    }
                }
            }
            // 3. periodically attempt to connect to any allowed routers that we
            // are not connected to -- TODO do some exponential backoff if a router
            // is not responding.
            _ = time::sleep(router_reconnect_delay) => {
                router_reconnect_delay = std::time::Duration::from_secs(4);
                tokio::spawn(connect_to_routers(
                    our.clone(),
                    our_ip.clone(),
                    keypair.clone(),
                    pki.clone(),
                    peers.clone(),
                    kernel_message_tx.clone(),
                    print_tx.clone()
                ));
            }
        }
    }
}

#[cfg(not(feature = "simulation-mode"))]
async fn connect_to_routers(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    pki: OnchainPKI,
    peers: Peers,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) -> Result<()> {
    for router in &our.allowed_routers {
        if peers.contains_key(router) {
            continue;
        }
        let Some(router_id) = pki.get(router) else {
            continue;
        };
        print_debug(
            &print_tx,
            &format!("net: attempting to connect to router {router}"),
        )
        .await;
        match init_connection(&our, &our_ip, &router_id, &keypair, None, true).await {
            Ok(direct_conn) => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("connected to router {}", router_id.name),
                    })
                    .await?;
                save_new_peer(
                    &router_id,
                    false,
                    peers.clone(),
                    direct_conn,
                    None,
                    &kernel_message_tx,
                    &print_tx,
                )
                .await;
            }
            Err(_e) => continue,
        }
    }
    Ok(())
}

#[cfg(not(feature = "simulation-mode"))]
async fn direct_networking(
    our: Identity,
    our_ip: String,
    tcp: TcpListener,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    _self_message_tx: MessageSender,
    mut message_rx: MessageReceiver,
    contract_address: String,
) -> Result<()> {
    print_debug(&print_tx, "net: starting as direct").await;
    let pki: OnchainPKI = Arc::new(DashMap::new());
    let peers: Peers = Arc::new(DashMap::new());
    // mapping from KNS namehash to username
    let names: PKINames = Arc::new(DashMap::new());
    // direct-specific structures
    let mut forwarding_connections = JoinSet::<()>::new();
    let mut pending_passthroughs: PendingPassthroughs = HashMap::new();
    // track peers that we're already in the midst of establishing a connection with
    let mut pending_connections = JoinSet::<(NodeId, Result<()>)>::new();
    let mut peer_message_queues = HashMap::<NodeId, Vec<KernelMessage>>::new();

    loop {
        tokio::select! {
            // 1. receive messages from kernel and send out over our connections,
            // making new connections as needed
            Some(km) = message_rx.recv() => {
                // got a message from kernel to send out over the network
                // if the message is for us, it's either a protocol-level "hello" message,
                // or a debugging command issued from our terminal. handle it here:
                if km.target.node == our.name {
                    match handle_local_message(
                        &our,
                        &our_ip,
                        &keypair,
                        km,
                        peers.clone(),
                        pki.clone(),
                        Some(&mut pending_passthroughs),
                        Some(&forwarding_connections),
                        names.clone(),
                        &kernel_message_tx,
                        &print_tx,
                        &contract_address,
                    )
                    .await {
                        Ok(()) => continue,
                        Err(e) => {
                            print_tx.send(Printout {
                                verbosity: 2,
                                content: format!("net: error handling local message: {}", e)
                            }).await?;
                            continue;
                        }
                    }
                }
                // if the message is for a peer we currently have a connection with,
                // try to send it to them
                else if let Some(peer) = peers.get_mut(&km.target.node) {
                    peer.sender.send(km)?;
                    continue
                }
                // if we cannot send it to an existing peer-connection, need to spawn
                // a task that will attempt to establish such a connection.
                // if such a task already exists for that peer, we should queue the message
                // to be sent once that task completes. otherwise, it will duplicate connections.
                pending_connections.spawn(establish_new_peer_connection(
                    our.clone(),
                    our_ip.clone(),
                    keypair.clone(),
                    km,
                    pki.clone(),
                    names.clone(),
                    peers.clone(),
                    true,
                    kernel_message_tx.clone(),
                    network_error_tx.clone(),
                    print_tx.clone()
                ));
            }
            // 2. recover the result of a pending connection and flush any message
            // queue that's built up since it was spawned
            Some(Ok((peer_name, result))) = pending_connections.join_next() => {
                match result {
                    Ok(()) => {
                        // if we have a message queue for this peer, send it out
                        if let Some(queue) = peer_message_queues.remove(&peer_name) {
                            for km in queue {
                                peers.get_mut(&peer_name).unwrap().sender.send(km)?;
                            }
                        }
                    }
                    Err(_e) => {
                        // TODO decide if this is good behavior, but throw
                        // offline error for each message in this peer's queue
                        if let Some(queue) = peer_message_queues.remove(&peer_name) {
                            for km in queue {
                                error_offline(km, &network_error_tx).await?;
                            }
                        }
                    }
                }
            }
            // 3. join any closed forwarding connection tasks and destroy them
            // TODO can do more here if desired
            Some(res) = forwarding_connections.join_next() => {
                match res {
                    Ok(()) => continue,
                    Err(_e) => continue,
                }
            }
            // 4. receive incoming TCP connections
            Ok((stream, _socket_addr)) = tcp.accept() => {
                // TODO we can perform some amount of validation here
                // to prevent some amount of potential DDoS attacks.
                // can also block based on socket_addr
                // ignore connections we failed to accept...?
                if let Ok(Ok(websocket)) = time::timeout(TIMEOUT, accept_async(MaybeTlsStream::Plain(stream))).await {
                    print_debug(&print_tx, "net: received new websocket connection").await;
                    let (peer_id, routing_for, conn) =
                        match time::timeout(TIMEOUT, recv_connection(
                            &our,
                            &our_ip,
                            &pki,
                            &peers,
                            &mut pending_passthroughs,
                            &keypair,
                            websocket)).await
                        {
                            Ok(Ok(res)) => res,
                            Ok(Err(e)) => {
                                print_tx.send(Printout {
                                    verbosity: 2,
                                    content: format!("net: recv_connection failed: {e}"),
                                }).await?;
                                continue;
                            }
                            Err(_e) => {
                                print_tx.send(Printout {
                                    verbosity: 2,
                                    content: "net: recv_connection timed out".into(),
                                }).await?;
                                continue;
                            }
                        };
                    // TODO if their handshake indicates they want us to proxy
                    // for them (aka act as a router for them) we can choose
                    // whether to do so here!
                    // if conn is direct, add peer. if passthrough, add to our
                    // forwarding connections joinset
                    match conn {
                        Connection::Peer(peer_conn) => {
                            save_new_peer(
                                &peer_id,
                                routing_for,
                                peers.clone(),
                                peer_conn,
                                None,
                                &kernel_message_tx,
                                &print_tx
                            ).await;
                        }
                        Connection::Passthrough(passthrough_conn) => {
                            forwarding_connections.spawn(maintain_passthrough(
                                passthrough_conn,
                            ));
                        }
                        Connection::PendingPassthrough(pending_conn) => {
                            pending_passthroughs.insert(
                                (peer_id.name.clone(), pending_conn.target.clone()),
                                pending_conn
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(not(feature = "simulation-mode"))]
async fn establish_new_peer_connection(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    km: KernelMessage,
    pki: OnchainPKI,
    names: PKINames,
    peers: Peers,
    reveal_ip: bool,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
) -> (NodeId, Result<()>) {
    if let Some(peer_id) = pki.get(&km.target.node) {
        // if the message is for a *direct* peer we don't have a connection with,
        // try to establish a connection with them
        // here, we can *choose* to use our routers so as not to reveal
        // networking information about ourselves to the target.
        if peer_id.ws_routing.is_some() && reveal_ip {
            print_debug(
                &print_tx,
                &format!("net: attempting to connect to {} directly", peer_id.name),
            )
            .await;
            match time::timeout(
                TIMEOUT,
                init_connection(&our, &our_ip, &peer_id, &keypair, None, false),
            )
            .await
            {
                Ok(Ok(direct_conn)) => {
                    save_new_peer(
                        &peer_id,
                        false,
                        peers,
                        direct_conn,
                        Some(km),
                        &kernel_message_tx,
                        &print_tx,
                    )
                    .await;
                    (peer_id.name.clone(), Ok(()))
                }
                _ => {
                    let _ = error_offline(km, &network_error_tx).await;
                    (
                        peer_id.name.clone(),
                        Err(anyhow!("failed to connect to peer")),
                    )
                }
            }
        }
        // if the message is for an *indirect* peer we don't have a connection with,
        // or we want to protect our node's physical networking details from non-routers,
        // do some routing: in a randomized order, go through their listed routers
        // on chain and try to get one of them to build a proxied connection to
        // this node for you
        else {
            print_debug(
                &print_tx,
                &format!("net: attempting to connect to {} via router", peer_id.name),
            )
            .await;
            let sent = time::timeout(
                TIMEOUT,
                init_connection_via_router(
                    &our,
                    &our_ip,
                    &keypair,
                    km.clone(),
                    &peer_id,
                    &pki,
                    &names,
                    peers,
                    kernel_message_tx.clone(),
                    print_tx.clone(),
                ),
            )
            .await;
            if sent.unwrap_or(false) {
                (peer_id.name.clone(), Ok(()))
            } else {
                // none of the routers worked!
                let _ = error_offline(km, &network_error_tx).await;
                (
                    peer_id.name.clone(),
                    Err(anyhow!("failed to connect to peer")),
                )
            }
        }
    }
    // peer cannot be found in PKI, throw an offline error
    else {
        let peer_name = km.target.node.clone();
        let _ = error_offline(km, &network_error_tx).await;
        (peer_name, Err(anyhow!("failed to connect to peer")))
    }
}

#[cfg(not(feature = "simulation-mode"))]
async fn init_connection_via_router(
    our: &Identity,
    our_ip: &str,
    keypair: &Ed25519KeyPair,
    km: KernelMessage,
    peer_id: &Identity,
    pki: &OnchainPKI,
    names: &PKINames,
    peers: Peers,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) -> bool {
    let routers_shuffled = {
        let mut routers = peer_id.allowed_routers.clone();
        routers.shuffle(&mut rand::thread_rng());
        routers
    };
    for router_namehash in &routers_shuffled {
        let Some(router_name) = names.get(router_namehash) else {
            continue;
        };
        let router_id = match pki.get(router_name.as_str()) {
            None => continue,
            Some(id) => id,
        };
        match init_connection(our, our_ip, peer_id, keypair, Some(&router_id), false).await {
            Ok(direct_conn) => {
                save_new_peer(
                    peer_id,
                    false,
                    peers,
                    direct_conn,
                    Some(km),
                    &kernel_message_tx,
                    &print_tx,
                )
                .await;
                return true;
            }
            Err(_) => continue,
        }
    }
    false
}

#[cfg(not(feature = "simulation-mode"))]
async fn recv_connection(
    our: &Identity,
    our_ip: &str,
    pki: &OnchainPKI,
    peers: &Peers,
    pending_passthroughs: &mut PendingPassthroughs,
    keypair: &Ed25519KeyPair,
    websocket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> Result<(Identity, bool, Connection)> {
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_responder();
    let (mut write_stream, mut read_stream) = websocket.split();

    // before we begin XX handshake pattern, check first message over socket
    let first_message = &ws_recv(&mut read_stream, &mut write_stream).await?;

    // if the first message contains a "routing request",
    // we see if the target is someone we are actively routing for,
    // and create a Passthrough connection if so.
    // a Noise 'e' message with have len 32
    if first_message.len() != 32 {
        let (their_id, target_name) = validate_routing_request(&our.name, first_message, pki)?;
        let (id, conn) = create_passthrough(
            our,
            our_ip,
            their_id,
            target_name,
            pki,
            peers,
            pending_passthroughs,
            write_stream,
            read_stream,
        )
        .await?;
        return Ok((id, false, conn));
    }

    // <- e
    noise.read_message(first_message, &mut buf)?;

    // -> e, ee, s, es
    send_protocol_handshake(
        our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake =
        recv_protocol_handshake(&mut noise, &mut buf, &mut read_stream, &mut write_stream).await?;

    // now validate this handshake payload against the KNS PKI
    let their_id = pki
        .get(&their_handshake.name)
        .ok_or(anyhow!("unknown KNS name"))?;
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        &their_id,
    )?;

    Ok((
        their_id.clone(),
        their_handshake.proxy_request,
        Connection::Peer(PeerConnection {
            noise: noise.into_transport_mode()?,
            buf,
            write_stream,
            read_stream,
        }),
    ))
}

#[cfg(not(feature = "simulation-mode"))]
async fn recv_connection_via_router(
    our: &Identity,
    our_ip: &str,
    their_name: &str,
    pki: &OnchainPKI,
    keypair: &Ed25519KeyPair,
    router: &Identity,
) -> Result<(Identity, PeerConnection)> {
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_responder();

    let Some((ref ip, ref port)) = router.ws_routing else {
        return Err(anyhow!("router has no routing information"));
    };
    let Ok(ws_url) = make_ws_url(our_ip, ip, port) else {
        return Err(anyhow!("failed to parse websocket url"));
    };
    let Ok(Ok((websocket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await else {
        return Err(anyhow!("failed to connect to target"));
    };
    let (mut write_stream, mut read_stream) = websocket.split();

    // before beginning XX handshake pattern, send a routing request
    let req = rmp_serde::to_vec(&RoutingRequest {
        protocol_version: 1,
        source: our.name.clone(),
        signature: keypair
            .sign([their_name, router.name.as_str()].concat().as_bytes())
            .as_ref()
            .to_vec(),
        target: their_name.to_string(),
    })?;
    write_stream.send(tungstenite::Message::binary(req)).await?;
    // <- e
    noise.read_message(
        &ws_recv(&mut read_stream, &mut write_stream).await?,
        &mut buf,
    )?;

    // -> e, ee, s, es
    send_protocol_handshake(
        our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake =
        recv_protocol_handshake(&mut noise, &mut buf, &mut read_stream, &mut write_stream).await?;

    // now validate this handshake payload against the KNS PKI
    let their_id = pki
        .get(&their_handshake.name)
        .ok_or(anyhow!("unknown KNS name"))?;
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        &their_id,
    )?;

    Ok((
        their_id.clone(),
        PeerConnection {
            noise: noise.into_transport_mode()?,
            buf,
            write_stream,
            read_stream,
        },
    ))
}

#[cfg(not(feature = "simulation-mode"))]
async fn init_connection(
    our: &Identity,
    our_ip: &str,
    peer_id: &Identity,
    keypair: &Ed25519KeyPair,
    use_router: Option<&Identity>,
    proxy_request: bool,
) -> Result<PeerConnection> {
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_initiator();

    let (ref ip, ref port) = match use_router {
        None => peer_id
            .ws_routing
            .as_ref()
            .ok_or(anyhow!("target has no routing information"))?,
        Some(router_id) => router_id
            .ws_routing
            .as_ref()
            .ok_or(anyhow!("target has no routing information"))?,
    };
    let ws_url = make_ws_url(our_ip, ip, port)?;
    let Ok(Ok((websocket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await else {
        return Err(anyhow!("failed to connect to target"));
    };
    let (mut write_stream, mut read_stream) = websocket.split();

    // if this is a routed request, before starting XX handshake pattern, send a
    // routing request message over socket
    if use_router.is_some() {
        let req = rmp_serde::to_vec(&RoutingRequest {
            protocol_version: 1,
            source: our.name.clone(),
            signature: keypair
                .sign(
                    [&peer_id.name, use_router.unwrap().name.as_str()]
                        .concat()
                        .as_bytes(),
                )
                .as_ref()
                .to_vec(),
            target: peer_id.name.clone(),
        })?;
        write_stream.send(tungstenite::Message::binary(req)).await?;
    }

    // -> e
    let len = noise.write_message(&[], &mut buf)?;
    write_stream
        .send(tungstenite::Message::binary(&buf[..len]))
        .await?;

    // <- e, ee, s, es
    let their_handshake =
        recv_protocol_handshake(&mut noise, &mut buf, &mut read_stream, &mut write_stream).await?;

    // now validate this handshake payload against the KNS PKI
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        peer_id,
    )?;

    // -> s, se
    send_protocol_handshake(
        our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
        proxy_request,
    )
    .await?;

    Ok(PeerConnection {
        noise: noise.into_transport_mode()?,
        buf,
        write_stream,
        read_stream,
    })
}

/// net module only handles incoming local requests, will never return a response
#[cfg(not(feature = "simulation-mode"))]
async fn handle_local_message(
    our: &Identity,
    our_ip: &str,
    keypair: &Ed25519KeyPair,
    km: KernelMessage,
    peers: Peers,
    pki: OnchainPKI,
    pending_passthroughs: Option<&mut PendingPassthroughs>,
    forwarding_connections: Option<&JoinSet<()>>,
    names: PKINames,
    kernel_message_tx: &MessageSender,
    print_tx: &PrintSender,
    contract_address: &str,
) -> Result<()> {
    print_debug(print_tx, "net: handling local message").await;
    let body = match km.message {
        Message::Request(ref request) => &request.body,
        Message::Response((response, _context)) => {
            // these are received as a router, when we send ConnectionRequests
            // to a node we do routing for.
            match rmp_serde::from_slice::<NetResponses>(&response.body) {
                Ok(NetResponses::Accepted(_)) => {
                    // TODO anything here?
                }
                Ok(NetResponses::Rejected(to)) => {
                    // drop from our pending map
                    // this will drop the socket, causing initiator to see it as failed
                    pending_passthroughs
                        .ok_or(anyhow!("got net response as non-router"))?
                        .remove(&(to, km.source.node));
                }
                Err(_) => {
                    // this is usually the "delivered" response to a raw message
                }
            }
            return Ok(());
        }
    };

    if km.source.node != our.name {
        if let Ok(act) = rmp_serde::from_slice::<NetActions>(body) {
            match act {
                NetActions::KnsBatchUpdate(_) | NetActions::KnsUpdate(_) => {
                    // for now, we don't get these from remote.
                }
                NetActions::ConnectionRequest(from) => {
                    // someone wants to open a passthrough with us through a router!
                    // if we are an indirect node, and source is one of our routers,
                    // respond by attempting to init a matching passthrough.
                    let res: Result<NetResponses> = if our.allowed_routers.contains(&km.source.node)
                    {
                        let router_id = peers
                            .get(&km.source.node)
                            .ok_or(anyhow!("unknown router"))?
                            .identity
                            .clone();
                        let (peer_id, peer_conn) = time::timeout(
                            TIMEOUT,
                            recv_connection_via_router(
                                our, our_ip, &from, &pki, keypair, &router_id,
                            ),
                        )
                        .await??;
                        save_new_peer(
                            &peer_id,
                            false,
                            peers,
                            peer_conn,
                            None,
                            kernel_message_tx,
                            print_tx,
                        )
                        .await;
                        Ok(NetResponses::Accepted(from.clone()))
                    } else {
                        Ok(NetResponses::Rejected(from.clone()))
                    };
                    kernel_message_tx
                        .send(KernelMessage {
                            id: km.id,
                            source: Address {
                                node: our.name.clone(),
                                process: ProcessId::new(Some("net"), "distro", "sys"),
                            },
                            target: km.rsvp.unwrap_or(km.source),
                            rsvp: None,
                            message: Message::Response((
                                Response {
                                    inherit: false,
                                    body: rmp_serde::to_vec(
                                        &res.unwrap_or(NetResponses::Rejected(from)),
                                    )?,
                                    metadata: None,
                                    capabilities: vec![],
                                },
                                None,
                            )),
                            lazy_load_blob: None,
                        })
                        .await?;
                }
            }
            return Ok(());
        };
        // if we can't parse this to a netaction, treat it as a hello and print it
        // respond to a text message with a simple "delivered" response
        parse_hello_message(our, &km, body, kernel_message_tx, print_tx).await?;
        Ok(())
    } else {
        // available commands: "peers", "pki", "names", "diagnostics"
        // first parse as raw string, then deserialize to NetActions object
        let mut printout = String::new();
        match rmp_serde::from_slice::<NetActions>(body) {
            Ok(NetActions::ConnectionRequest(_)) => {
                // we shouldn't receive these from ourselves.
            }
            Ok(NetActions::KnsUpdate(log)) => {
                pki.insert(
                    log.name.clone(),
                    Identity {
                        name: log.name.clone(),
                        networking_key: log.public_key,
                        ws_routing: if log.ip == *"0.0.0.0" || log.port == 0 {
                            None
                        } else {
                            Some((log.ip, log.port))
                        },
                        allowed_routers: log.routers,
                    },
                );
                names.insert(log.node, log.name);
            }
            Ok(NetActions::KnsBatchUpdate(log_list)) => {
                for log in log_list {
                    pki.insert(
                        log.name.clone(),
                        Identity {
                            name: log.name.clone(),
                            networking_key: log.public_key,
                            ws_routing: if log.ip == *"0.0.0.0" || log.port == 0 {
                                None
                            } else {
                                Some((log.ip, log.port))
                            },
                            allowed_routers: log.routers,
                        },
                    );
                    names.insert(log.node, log.name);
                }
            }
            _ => match std::str::from_utf8(body) {
                Ok("peers") => {
                    printout.push_str(&format!(
                        "{:#?}",
                        peers
                            .iter()
                            .map(|p| p.identity.name.clone())
                            .collect::<Vec<_>>()
                    ));
                }
                Ok("pki") => {
                    printout.push_str(&format!("{:#?}", pki));
                }
                Ok("names") => {
                    printout.push_str(&format!("{:#?}", names));
                }
                Ok("diagnostics") => {
                    printout.push_str(&format!(
                        "indexing from contract address {}\r\n",
                        contract_address
                    ));
                    printout.push_str(&format!("our Identity: {:#?}\r\n", our));
                    printout.push_str("we have connections with peers:\r\n");
                    for peer in peers.iter() {
                        printout.push_str(&format!(
                            "    {}, routing_for={}\r\n",
                            peer.identity.name, peer.routing_for,
                        ));
                    }
                    printout.push_str(&format!("we have {} entries in the PKI\r\n", pki.len()));
                    if pending_passthroughs.is_some() {
                        printout.push_str(&format!(
                            "we have {} pending passthrough connections\r\n",
                            pending_passthroughs.unwrap().len()
                        ));
                    }
                    if forwarding_connections.is_some() {
                        printout.push_str(&format!(
                            "we have {} open passthrough connections\r\n",
                            forwarding_connections.unwrap().len()
                        ));
                    }
                }
                Ok(other) => {
                    // parse non-commands as a request to fetch networking data
                    // about a specific node name
                    printout.push_str(&format!("net: printing known identity for {}\r\n", other));
                    match pki.get(other) {
                        Some(id) => {
                            printout.push_str(&format!("{:#?}", *id));
                        }
                        None => {
                            printout.push_str("no such identity known!");
                        }
                    }
                }
                _ => {}
            },
        }
        if !printout.is_empty() {
            if let Message::Request(req) = km.message {
                if req.expects_response.is_some() {
                    kernel_message_tx
                        .send(KernelMessage {
                            id: km.id,
                            source: Address {
                                node: our.name.clone(),
                                process: ProcessId::new(Some("net"), "distro", "sys"),
                            },
                            target: km.rsvp.unwrap_or(km.source),
                            rsvp: None,
                            message: Message::Response((
                                Response {
                                    inherit: false,
                                    body: printout.clone().into_bytes(),
                                    metadata: None,
                                    capabilities: vec![],
                                },
                                None,
                            )),
                            lazy_load_blob: None,
                        })
                        .await?;
                }
            }
            print_tx
                .send(Printout {
                    verbosity: 0,
                    content: printout,
                })
                .await?;
        }
        Ok(())
    }
}
