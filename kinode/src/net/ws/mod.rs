use crate::net::{
    types::{IdentityExt, NetData, OnchainPKI, PKINames, Peer, Peers, RoutingRequest, WS_PROTOCOL},
    utils::{
        build_initiator, build_responder, error_offline, make_conn_url, print_debug,
        validate_handshake, validate_routing_request, validate_signature,
    },
};
use lib::types::core::{
    Address, Identity, KernelMessage, LazyLoadBlob, Message, MessageReceiver, MessageSender,
    NetAction, NetResponse, NetworkErrorSender, NodeId, NodeRouting, PrintSender, Printout,
    ProcessId,
};
use {
    anyhow::{anyhow, Result},
    dashmap::DashMap,
    futures::{SinkExt, StreamExt},
    rand::seq::SliceRandom,
    ring::signature::Ed25519KeyPair,
    std::{collections::HashMap, sync::Arc},
    tokio::net::{TcpListener, TcpStream},
    tokio::task::JoinSet,
    tokio::{sync::mpsc, time},
    tokio_tungstenite::{
        accept_async, connect_async, tungstenite, MaybeTlsStream, WebSocketStream,
    },
};

mod utils;

/// only used in connection initialization, otherwise, nacks and Responses are only used for "timeouts"
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// 10 MB -- TODO analyze as desired, apps can always chunk data into many messages
/// note that this only applies to cross-network messages, not local ones.
pub const MESSAGE_MAX_SIZE: u32 = 10_485_800;

pub struct PeerConnection {
    pub noise: snow::TransportState,
    pub buf: Vec<u8>,
    pub socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

/// (from, target) -> from's socket
pub type PendingPassthroughs =
    Arc<DashMap<(NodeId, NodeId), WebSocketStream<MaybeTlsStream<TcpStream>>>>;

pub async fn receiver(ext: IdentityExt, net_data: NetData) -> Result<()> {
    let pending_passthroughs: PendingPassthroughs = Arc::new(DashMap::new());
    let ws_port = ext.our.get_protocol_port(WS_PROTOCOL).unwrap();
    let ws = match TcpListener::bind(format!("0.0.0.0:{ws_port}")).await {
        Ok(ws) => ws,
        Err(_e) => {
            return Err(anyhow::anyhow!(
                "net: fatal error: can't listen on port {ws_port}, update your KNS identity or free up that port"
            ));
        }
    };

    print_debug(&ext.print_tx, &format!("net: listening on port {ws_port}")).await;

    loop {
        match ws.accept().await {
            Err(e) => {
                print_debug(
                    &ext.print_tx,
                    &format!("net: error accepting WS connection: {e}"),
                )
                .await;
            }
            Ok((stream, socket_addr)) => {
                print_debug(
                    &ext.print_tx,
                    &format!("net: got WS connection from {socket_addr}"),
                )
                .await;
                let ext = ext.clone();
                let net_data = net_data.clone();
                let pending_passthroughs = pending_passthroughs.clone();
                tokio::spawn(async move {
                    let Ok(Ok(websocket)) =
                        time::timeout(TIMEOUT, accept_async(MaybeTlsStream::Plain(stream))).await
                    else {
                        return;
                    };
                    match time::timeout(
                        TIMEOUT,
                        recv_connection(ext.clone(), net_data, pending_passthroughs, websocket),
                    )
                    .await
                    {
                        Ok(Ok(())) => return,
                        Ok(Err(e)) => {
                            print_debug(
                                &ext.print_tx,
                                &format!("net: error receiving TCP connection: {e}"),
                            )
                            .await
                        }
                        Err(_e) => {
                            print_debug(
                                &ext.print_tx,
                                &format!("net: TCP connection from {socket_addr} timed out"),
                            )
                            .await
                        }
                    }
                });
            }
        }
    }
}

pub async fn init_direct(
    ext: &IdentityExt,
    data: &NetData,
    peer_id: &Identity,
    port: u16,
    proxy_request: bool,
    peer_rx: mpsc::UnboundedReceiver<KernelMessage>,
) -> Result<(), mpsc::UnboundedReceiver<KernelMessage>> {
    todo!()
}

pub async fn init_routed(
    ext: &IdentityExt,
    data: &NetData,
    peer_id: &Identity,
    router_id: &Identity,
    port: u16,
    peer_rx: mpsc::UnboundedReceiver<KernelMessage>,
) -> Result<(), mpsc::UnboundedReceiver<KernelMessage>> {
    todo!()
}

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
        if peer_id.is_direct() && reveal_ip {
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
                    todo!();
                    // utils::save_new_peer(
                    //     &peer_id,
                    //     false,
                    //     peers,
                    //     direct_conn,
                    //     Some(km),
                    //     &kernel_message_tx,
                    //     &print_tx,
                    // )
                    // .await;
                    (peer_id.name.clone(), Ok(()))
                }
                _ => {
                    error_offline(km, &network_error_tx).await;
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
                error_offline(km, &network_error_tx).await;
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
        error_offline(km, &network_error_tx).await;
        (peer_name, Err(anyhow!("failed to connect to peer")))
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
    peers: Peers,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) -> bool {
    let routers_shuffled = {
        let mut routers = match our.routing {
            NodeRouting::Routers(ref routers) => routers.clone(),
            _ => vec![],
        };
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
                todo!();
                // utils::save_new_peer(
                //     peer_id,
                //     false,
                //     peers,
                //     direct_conn,
                //     Some(km),
                //     &kernel_message_tx,
                //     &print_tx,
                // )
                // .await;
                return true;
            }
            Err(_) => continue,
        }
    }
    false
}

async fn recv_connection(
    ext: IdentityExt,
    data: NetData,
    mut pending_passthroughs: PendingPassthroughs,
    mut socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> anyhow::Result<()> {
    let (mut noise, our_static_key) = build_responder();

    // before we begin XX handshake pattern, check first message over socket
    let first_message = &utils::ws_recv(&mut socket).await?;

    // if the first message contains a "routing request",
    // we see if the target is someone we are actively routing for,
    // and create a Passthrough connection if so.
    // a Noise 'e' message with have len 32
    if first_message.len() != 32 {
        let (from_id, target_id) =
            validate_routing_request(&ext.our.name, first_message, &data.pki)?;
        return utils::create_passthrough(
            &ext.our,
            &ext.our_ip,
            from_id,
            target_id,
            &data.peers,
            &mut pending_passthroughs,
            socket,
        )
        .await;
    }

    let mut buf = vec![0u8; 65535];

    // <- e
    noise.read_message(first_message, &mut buf)?;

    // -> e, ee, s, es
    utils::send_protocol_handshake(
        &ext.our,
        &ext.keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut socket,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake = utils::recv_protocol_handshake(&mut noise, &mut buf, &mut socket).await?;

    // now validate this handshake payload against the KNS PKI
    let their_id = data
        .pki
        .get(&their_handshake.name)
        .ok_or(anyhow!("unknown KNS name"))?;
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        &their_id,
    )?;

    let (peer_tx, peer_rx) = mpsc::unbounded_channel();
    data.peers.insert(
        their_id.name.clone(),
        Peer {
            identity: their_id.clone(),
            routing_for: their_handshake.proxy_request,
            sender: peer_tx,
        },
    );
    tokio::spawn(utils::maintain_connection(
        their_handshake.name,
        data.peers,
        PeerConnection {
            noise: noise.into_transport_mode()?,
            buf,
            socket,
        },
        peer_rx,
        ext.kernel_message_tx,
        ext.print_tx,
    ));
    Ok(())
}

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

    let Some((ip, port)) = router.ws_routing() else {
        return Err(anyhow!("router has no routing information"));
    };
    let Ok(ws_url) = make_conn_url(our_ip, ip, port, "ws") else {
        return Err(anyhow!("failed to parse websocket url"));
    };
    let Ok(Ok((mut socket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await
    else {
        return Err(anyhow!("failed to connect to target"));
    };
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
    socket.send(tungstenite::Message::binary(req)).await?;
    // <- e
    noise.read_message(&utils::ws_recv(&mut socket).await?, &mut buf)?;

    // -> e, ee, s, es
    utils::send_protocol_handshake(
        our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut socket,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake = utils::recv_protocol_handshake(&mut noise, &mut buf, &mut socket).await?;

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
            socket,
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
) -> Result<PeerConnection> {
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_initiator();

    let (ref ip, ref port) = match use_router {
        None => peer_id
            .ws_routing()
            .ok_or(anyhow!("target has no routing information"))?,
        Some(router_id) => router_id
            .ws_routing()
            .ok_or(anyhow!("target has no routing information"))?,
    };
    let ws_url = make_conn_url(our_ip, ip, port, "ws")?;
    let Ok(Ok((mut socket, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await
    else {
        return Err(anyhow!("failed to connect to target"));
    };

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
        socket.send(tungstenite::Message::binary(req)).await?;
    }

    // -> e
    let len = noise.write_message(&[], &mut buf)?;
    socket
        .send(tungstenite::Message::binary(&buf[..len]))
        .await?;

    // <- e, ee, s, es
    let their_handshake = utils::recv_protocol_handshake(&mut noise, &mut buf, &mut socket).await?;

    // now validate this handshake payload against the KNS PKI
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        peer_id,
    )?;

    // -> s, se
    utils::send_protocol_handshake(
        our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut socket,
        proxy_request,
    )
    .await?;

    Ok(PeerConnection {
        noise: noise.into_transport_mode()?,
        buf,
        socket,
    })
}
