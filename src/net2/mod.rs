use crate::net2::{types::*, utils::*};
use crate::types::*;
use anyhow::{anyhow, Result};
use futures::{SinkExt, StreamExt};
use rand::seq::SliceRandom;
use ring::signature::Ed25519KeyPair;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::task::JoinSet;
use tokio::time;
use tokio_tungstenite::{accept_async, connect_async, MaybeTlsStream, WebSocketStream};

mod types;
mod utils;

// only used in connection initialization, otherwise, nacks and Responses are only used for "timeouts"
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// 10 MB -- TODO analyze as desired, apps can always chunk data into many messages
const MESSAGE_MAX_SIZE: u32 = 10_485_800;

/// Entry point from the main kernel task. Runs forever, spawns listener and sender tasks.
pub async fn networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    message_rx: MessageReceiver,
) -> Result<()> {
    println!("networking!\r");
    println!("our identity: {:#?}\r", our);
    // branch on whether we are a direct or indirect node
    match &our.ws_routing {
        None => {
            // indirect node: run the indirect networking strategy
            indirect_networking(
                our,
                our_ip,
                keypair,
                kernel_message_tx,
                network_error_tx,
                print_tx,
                self_message_tx,
                message_rx,
            )
            .await
        }
        Some((ip, port)) => {
            // direct node: run the direct networking strategy
            if &our_ip != ip {
                return Err(anyhow!(
                    "net: fatal error: IP address mismatch: {} != {}, update your QNS identity",
                    our_ip,
                    ip
                ));
            }
            let tcp = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
                Ok(tcp) => tcp,
                Err(_e) => {
                    return Err(anyhow!(
                        "net: fatal error: can't listen on port {}, update your QNS identity or free up that port",
                        port,
                    ));
                }
            };
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
            )
            .await
        }
    }
}

async fn indirect_networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    mut message_rx: MessageReceiver,
) -> Result<()> {
    println!("indirect_networking\r");
    let mut pki: OnchainPKI = HashMap::new();
    let mut peers: Peers = HashMap::new();
    // mapping from QNS namehash to username
    let mut names: PKINames = HashMap::new();

    let mut peer_connections = JoinSet::<(NodeId, Option<KernelMessage>)>::new();
    let mut active_routers = HashSet::<NodeId>::new();

    // before opening up the main loop, go through our allowed routers
    // and attempt to connect to all of them, saving the successfully
    // connected-to ones in our router-set
    connect_to_routers(
        &our,
        &our_ip,
        &keypair,
        &mut active_routers,
        &pki,
        &mut peers,
        &mut peer_connections,
        kernel_message_tx.clone(),
    )
    .await;

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
                        &mut peers,
                        &mut pki,
                        &mut peer_connections,
                        None,
                        None,
                        Some(&active_routers),
                        &mut names,
                        &kernel_message_tx,
                        &print_tx,
                    )
                    .await {
                        Ok(()) => {},
                        Err(e) => {
                            print_tx.send(Printout {
                                verbosity: 0,
                                content: format!("net: error handling local message: {}", e)
                            }).await?;
                        }
                    }
                }
                // if the message is for a peer we currently have a connection with,
                // try to send it to them
                else if let Some(peer) = peers.get_mut(target) {
                    peer.sender.send(km)?;
                }
                else if let Some(peer_id) = pki.get(target) {
                    // if the message is for a *direct* peer we don't have a connection with,
                    // try to establish a connection with them
                    // TODO: here, we can *choose* to use our routers so as not to reveal
                    // networking information about ourselves to the target.
                    if peer_id.ws_routing.is_some() {
                        match init_connection(&our, &our_ip, peer_id, &keypair, None, false).await {
                            Ok((peer_name, direct_conn)) => {
                                let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                                let peer = Arc::new(Peer {
                                    identity: peer_id.clone(),
                                    routing_for: false,
                                    sender: peer_tx,
                                });
                                peers.insert(peer_name, peer.clone());
                                peer.sender.send(km)?;
                                peer_connections.spawn(maintain_connection(
                                    peer,
                                    direct_conn,
                                    peer_rx,
                                    kernel_message_tx.clone(),
                                ));
                            }
                            Err(e) => {
                                println!("net: error initializing connection: {}\r", e);
                                error_offline(km, &network_error_tx).await?;
                            }
                        }
                    }
                    // if the message is for an *indirect* peer we don't have a connection with,
                    // do some routing: in a randomized order, go through their listed routers
                    // on chain and try to get one of them to build a proxied connection to
                    // this node for you
                    else {
                        let sent = time::timeout(TIMEOUT,
                            init_connection_via_router(
                                &our,
                                &our_ip,
                                &keypair,
                                km.clone(),
                                peer_id,
                                &pki,
                                &names,
                                &mut peers,
                                &mut peer_connections,
                                kernel_message_tx.clone()
                            )).await;
                        if !sent.unwrap_or(false) {
                            // none of the routers worked!
                            println!("net: error initializing routed connection\r");
                            error_offline(km, &network_error_tx).await?;
                        }
                    }
                }
                // peer cannot be found in PKI, throw an offline error
                else {
                    error_offline(km, &network_error_tx).await?;
                }
            }
            // 2. deal with active connections that die by removing the associated peer
            // if the peer is one of our routers, remove them from router-set
            Some(Ok((dead_peer, maybe_resend))) = peer_connections.join_next() => {
                peers.remove(&dead_peer);
                active_routers.remove(&dead_peer);
                match maybe_resend {
                    None => {},
                    Some(km) => {
                        self_message_tx.send(km).await?;
                    }
                }
            }
            // 3. periodically attempt to connect to any allowed routers that we
            // are not connected to
            _ = time::sleep(time::Duration::from_secs(3)) => {
                connect_to_routers(
                    &our,
                    &our_ip,
                    &keypair,
                    &mut active_routers,
                    &pki,
                    &mut peers,
                    &mut peer_connections,
                    kernel_message_tx.clone(),
                )
                .await;
            }
        }
    }
}

async fn connect_to_routers(
    our: &Identity,
    our_ip: &str,
    keypair: &Ed25519KeyPair,
    active_routers: &mut HashSet<NodeId>,
    pki: &OnchainPKI,
    peers: &mut Peers,
    peer_connections: &mut JoinSet<(NodeId, Option<KernelMessage>)>,
    kernel_message_tx: MessageSender,
) {
    for router in &our.allowed_routers {
        if active_routers.contains(router) {
            continue;
        }
        let Some(router_id) = pki.get(router) else {
            continue;
        };
        match init_connection(our, our_ip, router_id, keypair, None, true).await {
            Ok((peer_name, direct_conn)) => {
                let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                let peer = Arc::new(Peer {
                    identity: router_id.clone(),
                    routing_for: false,
                    sender: peer_tx,
                });
                println!("net: connected to router {}\r", peer_name);
                peers.insert(peer_name.clone(), peer.clone());
                active_routers.insert(peer_name);
                peer_connections.spawn(maintain_connection(
                    peer,
                    direct_conn,
                    peer_rx,
                    kernel_message_tx.clone(),
                ));
            }
            Err(_e) => continue,
        }
    }
}

async fn direct_networking(
    our: Identity,
    our_ip: String,
    tcp: TcpListener,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    mut message_rx: MessageReceiver,
) -> Result<()> {
    println!("direct_networking\r");
    let mut pki: OnchainPKI = HashMap::new();
    let mut peers: Peers = HashMap::new();
    // mapping from QNS namehash to username
    let mut names: PKINames = HashMap::new();

    let mut peer_connections = JoinSet::<(NodeId, Option<KernelMessage>)>::new();
    let mut forwarding_connections = JoinSet::<()>::new();
    let mut pending_passthroughs: PendingPassthroughs = HashMap::new();

    loop {
        tokio::select! {
            // 1. receive messages from kernel and send out over our connections,
            // making new connections as needed
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
                        &mut peers,
                        &mut pki,
                        &mut peer_connections,
                        Some(&mut pending_passthroughs),
                        Some(&forwarding_connections),
                        None,
                        &mut names,
                        &kernel_message_tx,
                        &print_tx,
                    )
                    .await {
                        Ok(()) => {},
                        Err(e) => {
                            print_tx.send(Printout {
                                verbosity: 0,
                                content: format!("net: error handling local message: {}", e)
                            }).await?;
                        }
                    }
                }
                // if the message is for a peer we currently have a connection with,
                // try to send it to them
                else if let Some(peer) = peers.get_mut(target) {
                    peer.sender.send(km)?;
                }
                else if let Some(peer_id) = pki.get(target) {
                    // if the message is for a *direct* peer we don't have a connection with,
                    // try to establish a connection with them
                    if peer_id.ws_routing.is_some() {
                        match init_connection(&our, &our_ip, peer_id, &keypair, None, false).await {
                            Ok((peer_name, direct_conn)) => {
                                let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                                let peer = Arc::new(Peer {
                                    identity: peer_id.clone(),
                                    routing_for: false,
                                    sender: peer_tx,
                                });
                                peers.insert(peer_name, peer.clone());
                                peer.sender.send(km)?;
                                peer_connections.spawn(maintain_connection(
                                    peer,
                                    direct_conn,
                                    peer_rx,
                                    kernel_message_tx.clone(),
                                ));
                            }
                            Err(e) => {
                                println!("net: error initializing connection: {}\r", e);
                                error_offline(km, &network_error_tx).await?;
                            }
                        }
                    }
                    // if the message is for an *indirect* peer we don't have a connection with,
                    // do some routing: in a randomized order, go through their listed routers
                    // on chain and try to get one of them to build a proxied connection to
                    // this node for you
                    else {
                        let sent = time::timeout(TIMEOUT,
                            init_connection_via_router(
                                &our,
                                &our_ip,
                                &keypair,
                                km.clone(),
                                peer_id,
                                &pki,
                                &names,
                                &mut peers,
                                &mut peer_connections,
                                kernel_message_tx.clone()
                            )).await;
                        if !sent.unwrap_or(false) {
                            // none of the routers worked!
                            println!("net: error initializing routed connection\r");
                            error_offline(km, &network_error_tx).await?;
                        }
                    }
                }
                // peer cannot be found in PKI, throw an offline error
                else {
                    error_offline(km, &network_error_tx).await?;
                }
            }
            // 2. receive incoming TCP connections
            Ok((stream, _socket_addr)) = tcp.accept() => {
                // TODO we can perform some amount of validation here
                // to prevent some amount of potential DDoS attacks.
                // can also block based on socket_addr
                match accept_async(MaybeTlsStream::Plain(stream)).await {
                    Ok(websocket) => {
                        let (peer_id, routing_for, conn) =
                            match recv_connection(
                                &our,
                                &our_ip,
                                &pki,
                                &peers,
                                &mut pending_passthroughs,
                                &keypair,
                                websocket).await
                            {
                                Ok(res) => res,
                                Err(e) => {
                                    println!("net: recv_connection failed: {e}\r");
                                    continue;
                                }
                            };
                        // if conn is direct, add peer
                        // if passthrough, add to our forwarding connections joinset
                        match conn {
                            Connection::Peer(peer_conn) => {
                                let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                                let peer = Arc::new(Peer {
                                    identity: peer_id,
                                    routing_for,
                                    sender: peer_tx,
                                });
                                peers.insert(peer.identity.name.clone(), peer.clone());
                                peer_connections.spawn(maintain_connection(
                                    peer,
                                    peer_conn,
                                    peer_rx,
                                    kernel_message_tx.clone(),
                                ));
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
                    // ignore connections we failed to accept...?
                    Err(_) => {}
                }
            }
            // 3. deal with active connections that die by removing the associated peer
            Some(Ok((dead_peer, maybe_resend))) = peer_connections.join_next() => {
                peers.remove(&dead_peer);
                match maybe_resend {
                    None => {},
                    Some(km) => {
                        self_message_tx.send(km).await?;
                    }
                }
            }
        }
    }
}

async fn init_connection_via_router(
    our: &Identity,
    our_ip: &str,
    keypair: &Ed25519KeyPair,
    km: KernelMessage,
    peer_id: &Identity,
    pki: &OnchainPKI,
    names: &PKINames,
    peers: &mut Peers,
    peer_connections: &mut JoinSet<(NodeId, Option<KernelMessage>)>,
    kernel_message_tx: MessageSender,
) -> bool {
    println!("init_connection_via_router\r");
    let routers_shuffled = {
        let mut routers = peer_id.allowed_routers.clone();
        routers.shuffle(&mut rand::thread_rng());
        routers
    };
    for router_namehash in &routers_shuffled {
        let Some(router_name) = names.get(router_namehash) else {
            continue;
        };
        let router_id = match pki.get(router_name) {
            None => continue,
            Some(id) => id,
        };
        match init_connection(&our, &our_ip, peer_id, &keypair, Some(router_id), false).await {
            Ok((peer_name, direct_conn)) => {
                let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                let peer = Arc::new(Peer {
                    identity: peer_id.clone(),
                    routing_for: false,
                    sender: peer_tx,
                });
                peers.insert(peer_name, peer.clone());
                peer.sender.send(km).unwrap();
                peer_connections.spawn(maintain_connection(
                    peer,
                    direct_conn,
                    peer_rx,
                    kernel_message_tx.clone(),
                ));
                return true;
            }
            Err(_) => continue,
        }
    }
    return false;
}

async fn maintain_connection(
    peer: Arc<Peer>,
    mut conn: PeerConnection,
    mut peer_rx: UnboundedReceiver<KernelMessage>,
    kernel_message_tx: MessageSender,
    // network_error_tx: NetworkErrorSender,
    // print_tx: PrintSender,
) -> (NodeId, Option<KernelMessage>) {
    println!("maintain_connection\r");
    loop {
        tokio::select! {
            recv_result = recv_uqbar_message(&mut conn) => {
                match recv_result {
                    Ok(km) => {
                        if km.source.node != peer.identity.name {
                            println!("net: got message with spoofed source\r");
                            return (peer.identity.name.clone(), None)
                        }
                        kernel_message_tx.send(km).await.expect("net error: fatal: kernel died");
                    }
                    Err(e) => {
                        println!("net: error receiving message: {}\r", e);
                        return (peer.identity.name.clone(), None)
                    }
                }
            },
            maybe_recv = peer_rx.recv() => {
                match maybe_recv {
                    Some(km) => {
                        // TODO error handle
                        match send_uqbar_message(&km, &mut conn).await {
                            Ok(()) => continue,
                            Err(e) => {
                                println!("net: error sending message: {}\r", e);
                                return (peer.identity.name.clone(), Some(km))
                            }
                        }
                    }
                    None => {
                        println!("net: peer disconnected\r");
                        return (peer.identity.name.clone(), None)
                    }
                }
            },
        }
    }
}

/// match the streams
/// TODO optimize performance of this
async fn maintain_passthrough(mut conn: PassthroughConnection) {
    println!("maintain_passthrough\r");
    loop {
        tokio::select! {
            maybe_recv = conn.read_stream_1.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        conn.write_stream_2.send(msg).await.expect("net error: fatal: kernel died");
                    }
                    _ => {
                        println!("net: passthrough broke\r");
                        return
                    }
                }
            },
            maybe_recv = conn.read_stream_2.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        conn.write_stream_1.send(msg).await.expect("net error: fatal: kernel died");
                    }
                    _ => {
                        println!("net: passthrough broke\r");
                        return
                    }
                }
            },
        }
    }
}

async fn recv_connection(
    our: &Identity,
    our_ip: &str,
    pki: &OnchainPKI,
    peers: &Peers,
    pending_passthroughs: &mut PendingPassthroughs,
    keypair: &Ed25519KeyPair,
    websocket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> Result<(Identity, bool, Connection)> {
    println!("recv_connection\r");
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_responder();
    let (mut write_stream, mut read_stream) = websocket.split();

    // before we begin XX handshake pattern, check first message over socket
    let first_message = &ws_recv(&mut read_stream).await?;

    // if the first message contains a "routing request",
    // we see if the target is someone we are actively routing for,
    // and create a Passthrough connection if so.
    // a Noise 'e' message with have len 32
    if first_message.len() != 32 {
        let (their_id, target_name) = validate_routing_request(&our.name, &first_message, pki)?;
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
    send_uqbar_handshake(
        &our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake = recv_uqbar_handshake(&mut noise, &mut buf, &mut read_stream).await?;

    // now validate this handshake payload against the QNS PKI
    let their_id = pki
        .get(&their_handshake.name)
        .ok_or(anyhow!("unknown QNS name"))?;
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        their_id,
    )?;

    // Transition the state machine into transport mode now that the handshake is complete.
    let noise = noise.into_transport_mode()?;
    println!("handshake complete, noise session received\r");

    // TODO if their handshake indicates they want us to proxy
    // for them (aka act as a router for them) we can choose
    // whether to do so here.
    Ok((
        their_id.clone(),
        their_handshake.proxy_request,
        Connection::Peer(PeerConnection {
            noise,
            buf,
            write_stream,
            read_stream,
        }),
    ))
}

async fn recv_connection_via_router(
    our: &Identity,
    our_ip: &str,
    their_name: &str,
    pki: &OnchainPKI,
    keypair: &Ed25519KeyPair,
    router: &Identity,
) -> Result<(Identity, PeerConnection)> {
    println!("recv_connection_via_router\r");
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_responder();

    let Some((ref ip, ref port)) = router.ws_routing else {
        return Err(anyhow!("router has no routing information"));
    };
    let Ok(ws_url) = make_ws_url(our_ip, ip, port) else {
        return Err(anyhow!("failed to parse websocket url"));
    };
    let Ok(Ok((websocket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await
    else {
        return Err(anyhow!("failed to connect to target"));
    };
    let (mut write_stream, mut read_stream) = websocket.split();

    // before beginning XX handshake pattern, send a routing request
    let message = bincode::serialize(&RoutingRequest {
        source: our.name.clone(),
        signature: keypair
            .sign([their_name, router.name.as_str()].concat().as_bytes())
            .as_ref()
            .to_vec(),
        target: their_name.to_string(),
        protocol_version: 1,
    })?;
    ws_send(&mut write_stream, &message).await?;

    // <- e
    noise.read_message(&ws_recv(&mut read_stream).await?, &mut buf)?;

    // -> e, ee, s, es
    send_uqbar_handshake(
        &our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake = recv_uqbar_handshake(&mut noise, &mut buf, &mut read_stream).await?;

    // now validate this handshake payload against the QNS PKI
    let their_id = pki
        .get(&their_handshake.name)
        .ok_or(anyhow!("unknown QNS name"))?;
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        their_id,
    )?;

    // Transition the state machine into transport mode now that the handshake is complete.
    let noise = noise.into_transport_mode()?;
    println!("handshake complete, noise session received\r");

    Ok((
        their_id.clone(),
        PeerConnection {
            noise,
            buf,
            write_stream,
            read_stream,
        },
    ))
}

async fn init_connection(
    our: &Identity,
    our_ip: &str,
    peer_id: &Identity,
    keypair: &Ed25519KeyPair,
    use_router: Option<&Identity>,
    proxy_request: bool,
) -> Result<(String, PeerConnection)> {
    println!("init_connection\r");
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_initiator();

    let (mut write_stream, mut read_stream) = match use_router {
        None => {
            let Some((ref ip, ref port)) = peer_id.ws_routing else {
                return Err(anyhow!("target has no routing information"));
            };
            let Ok(ws_url) = make_ws_url(our_ip, ip, port) else {
                return Err(anyhow!("failed to parse websocket url"));
            };
            let Ok(Ok((websocket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await
            else {
                return Err(anyhow!("failed to connect to target"));
            };
            websocket.split()
        }
        Some(router_id) => {
            let Some((ref ip, ref port)) = router_id.ws_routing else {
                return Err(anyhow!("router has no routing information"));
            };
            let Ok(ws_url) = make_ws_url(our_ip, ip, port) else {
                return Err(anyhow!("failed to parse websocket url"));
            };
            let Ok(Ok((websocket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await
            else {
                return Err(anyhow!("failed to connect to target"));
            };
            websocket.split()
        }
    };

    // if this is a routed request, before starting XX handshake pattern, send a
    // routing request message over socket
    if use_router.is_some() {
        let message = bincode::serialize(&RoutingRequest {
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
            protocol_version: 1,
        })?;
        ws_send(&mut write_stream, &message).await?;
    }

    // -> e
    let len = noise.write_message(&[], &mut buf)?;
    ws_send(&mut write_stream, &buf[..len]).await?;

    // <- e, ee, s, es
    let their_handshake = recv_uqbar_handshake(&mut noise, &mut buf, &mut read_stream).await?;

    // now validate this handshake payload against the QNS PKI
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        peer_id,
    )?;

    // -> s, se
    send_uqbar_handshake(
        &our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
        proxy_request,
    )
    .await?;

    let noise = noise.into_transport_mode()?;
    println!("handshake complete, noise session initiated\r");

    Ok((
        their_handshake.name,
        PeerConnection {
            noise,
            buf,
            write_stream,
            read_stream,
        },
    ))
}

/// net module only handles incoming local requests, will never return a response
async fn handle_local_message(
    our: &Identity,
    our_ip: &str,
    keypair: &Ed25519KeyPair,
    km: KernelMessage,
    peers: &mut Peers,
    pki: &mut OnchainPKI,
    peer_connections: &mut JoinSet<(NodeId, Option<KernelMessage>)>,
    pending_passthroughs: Option<&mut PendingPassthroughs>,
    forwarding_connections: Option<&JoinSet<()>>,
    active_routers: Option<&HashSet<NodeId>>,
    names: &mut PKINames,
    kernel_message_tx: &MessageSender,
    print_tx: &PrintSender,
) -> Result<()> {
    println!("handle_local_message\r");
    let ipc = match km.message {
        Message::Request(request) => request.ipc,
        Message::Response((response, _context)) => {
            // these are received as a router, when we send ConnectionRequests
            // to a node we do routing for.
            match serde_json::from_slice::<NetResponses>(&response.ipc)? {
                NetResponses::Attempting(_) => {
                    // TODO anything here?
                }
                NetResponses::Rejected(to) => {
                    // drop from our pending map
                    // this will drop the socket, causing initiator to see it as failed
                    pending_passthroughs
                        .ok_or(anyhow!("got net response as non-router"))?
                        .remove(&(to, km.source.node));
                }
            }
            return Ok(());
        }
    };

    if km.source.node != our.name {
        if let Ok(act) = serde_json::from_slice::<NetActions>(&ipc) {
            match act {
                NetActions::QnsBatchUpdate(_) | NetActions::QnsUpdate(_) => {
                    // for now, we don't get these from remote.
                }
                NetActions::ConnectionRequest(from) => {
                    // someone wants to open a passthrough with us through a router!
                    // if we are an indirect node, and source is one of our routers,
                    // respond by attempting to init a matching passthrough.
                    // TODO can discriminate more here..
                    if our.allowed_routers.contains(&km.source.node) {
                        let Ok((peer_id, peer_conn)) = time::timeout(TIMEOUT,
                                recv_connection_via_router(
                                    our,
                                    our_ip,
                                    &from,
                                    pki,
                                    keypair,
                                    &peers
                                        .get(&km.source.node)
                                        .ok_or(anyhow!("unknown router"))?
                                        .identity,
                                )).await? else {
                                    return Err(anyhow!("someone tried to connect to us but it timed out"))
                                };
                        let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                        let peer = Arc::new(Peer {
                            identity: peer_id,
                            routing_for: false,
                            sender: peer_tx,
                        });
                        peers.insert(peer.identity.name.clone(), peer.clone());
                        peer_connections.spawn(maintain_connection(
                            peer,
                            peer_conn,
                            peer_rx,
                            kernel_message_tx.clone(),
                        ));
                    } else {
                        kernel_message_tx
                            .send(KernelMessage {
                                id: km.id,
                                source: Address {
                                    node: our.name.clone(),
                                    process: ProcessId::from_str("net:sys:uqbar").unwrap(),
                                },
                                target: km.rsvp.unwrap_or(km.source),
                                rsvp: None,
                                message: Message::Response((
                                    Response {
                                        inherit: false,
                                        ipc: serde_json::to_vec(&NetResponses::Rejected(from))?,
                                        metadata: None,
                                    },
                                    None,
                                )),
                                payload: None,
                                signed_capabilities: None,
                            })
                            .await?;
                    }
                }
            }
            return Ok(());
        };
        // if we can't parse this to a netaction, treat it as a hello and print it
        // respond to a text message with a simple "delivered" response
        print_tx
            .send(Printout {
                verbosity: 0,
                content: format!(
                    "\x1b[3;32m{}: {}\x1b[0m",
                    km.source.node,
                    std::str::from_utf8(&ipc).unwrap_or("!!message parse error!!")
                ),
            })
            .await?;
        kernel_message_tx
            .send(KernelMessage {
                id: km.id,
                source: Address {
                    node: our.name.clone(),
                    process: ProcessId::from_str("net:sys:uqbar").unwrap(),
                },
                target: km.rsvp.unwrap_or(km.source),
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        ipc: "delivered".as_bytes().to_vec(),
                        metadata: None,
                    },
                    None,
                )),
                payload: None,
                signed_capabilities: None,
            })
            .await?;
        Ok(())
    } else {
        // available commands: "peers", "pki", "names", "diagnostics"
        // first parse as raw string, then deserialize to NetActions object
        match std::str::from_utf8(&ipc) {
            Ok("peers") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("{:#?}", peers.keys()),
                    })
                    .await?;
            }
            Ok("pki") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("{:#?}", pki),
                    })
                    .await?;
            }
            Ok("names") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("{:#?}", names),
                    })
                    .await?;
            }
            Ok("diagnostics") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("our Identity: {:#?}", our),
                    })
                    .await?;
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("we have connections with peers: {:#?}", peers.keys()),
                    })
                    .await?;
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("we have {} entries in the PKI", pki.len()),
                    })
                    .await?;
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!(
                            "we have {} open peer connections",
                            peer_connections.len()
                        ),
                    })
                    .await?;
                if pending_passthroughs.is_some() {
                    print_tx
                        .send(Printout {
                            verbosity: 0,
                            content: format!(
                                "we have {} pending passthrough connections",
                                pending_passthroughs.unwrap().len()
                            ),
                        })
                        .await?;
                }
                if forwarding_connections.is_some() {
                    print_tx
                        .send(Printout {
                            verbosity: 0,
                            content: format!(
                                "we have {} open passthrough connections",
                                forwarding_connections.unwrap().len()
                            ),
                        })
                        .await?;
                }
                if active_routers.is_some() {
                    print_tx
                        .send(Printout {
                            verbosity: 0,
                            content: format!(
                                "we have {} active routers",
                                active_routers.unwrap().len()
                            ),
                        })
                        .await?;
                }
            }
            _ => {
                let Ok(act) = serde_json::from_slice::<NetActions>(&ipc) else {
                    print_tx
                        .send(Printout {
                            verbosity: 0,
                            content: "net: got unknown command".into(),
                        })
                        .await?;
                    return Ok(());
                };
                match act {
                    NetActions::ConnectionRequest(_) => {
                        // we shouldn't receive these from ourselves.
                    }
                    NetActions::QnsUpdate(log) => {
                        print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("net: got QNS update for {}", log.name),
                            })
                            .await?;

                        pki.insert(
                            log.name.clone(),
                            Identity {
                                name: log.name.clone(),
                                networking_key: log.public_key,
                                ws_routing: if log.ip == "0.0.0.0".to_string() || log.port == 0 {
                                    None
                                } else {
                                    Some((log.ip, log.port))
                                },
                                allowed_routers: log.routers,
                            },
                        );
                        names.insert(log.node, log.name);
                    }
                    NetActions::QnsBatchUpdate(log_list) => {
                        print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!(
                                    "net: got QNS update with {} peers",
                                    log_list.len()
                                ),
                            })
                            .await?;
                        for log in log_list {
                            pki.insert(
                                log.name.clone(),
                                Identity {
                                    name: log.name.clone(),
                                    networking_key: log.public_key,
                                    ws_routing: if log.ip == "0.0.0.0".to_string() || log.port == 0
                                    {
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
                }
            }
        }
        Ok(())
    }
}
