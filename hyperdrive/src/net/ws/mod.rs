use crate::net::{
    types::{IdentityExt, NetData, Peer, PendingStream, RoutingRequest, WS_PROTOCOL},
    utils::{
        build_initiator, build_responder, create_passthrough, make_conn_url, print_debug,
        validate_handshake, validate_routing_request, TIMEOUT,
    },
};
use lib::types::core::{Identity, KernelMessage};
use {
    anyhow::{anyhow, Result},
    futures::SinkExt,
    tokio::net::{TcpListener, TcpStream},
    tokio::{sync::mpsc, time},
    tokio_tungstenite::{
        accept_async, connect_async, tungstenite, MaybeTlsStream, WebSocketStream,
    },
};

mod utils;

pub struct PeerConnection {
    pub noise: snow::TransportState,
    pub buf: Vec<u8>,
    pub socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

pub type WebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn receiver(ext: IdentityExt, data: NetData) -> Result<()> {
    let ws_port = ext
        .our
        .get_protocol_port(WS_PROTOCOL)
        .expect("ws port not found");
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
                let ext = ext.clone();
                let data = data.clone();
                tokio::spawn(async move {
                    let Ok(Ok(websocket)) =
                        time::timeout(TIMEOUT, accept_async(MaybeTlsStream::Plain(stream))).await
                    else {
                        return;
                    };
                    print_debug(
                        &ext.print_tx,
                        &format!("net: got WS connection from {socket_addr}"),
                    )
                    .await;
                    match time::timeout(TIMEOUT, recv_connection(ext.clone(), data, websocket))
                        .await
                    {
                        Ok(Ok(())) => return,
                        Ok(Err(e)) => {
                            print_debug(
                                &ext.print_tx,
                                &format!("net: error receiving WS connection: {e}"),
                            )
                            .await
                        }
                        Err(_e) => {
                            print_debug(
                                &ext.print_tx,
                                &format!("net: WS connection from {socket_addr} timed out"),
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
    match time::timeout(
        TIMEOUT,
        connect_with_handshake(ext, peer_id, port, None, proxy_request),
    )
    .await
    {
        Ok(Ok(connection)) => {
            // maintain direct connection
            tokio::spawn(utils::maintain_connection(
                peer_id.name.clone(),
                data.peers.clone(),
                connection,
                peer_rx,
                ext.kernel_message_tx.clone(),
                ext.print_tx.clone(),
            ));
            Ok(())
        }
        Ok(Err(e)) => {
            print_debug(
                &ext.print_tx,
                &format!("net: error in ws::init_direct: {e}"),
            )
            .await;
            return Err(peer_rx);
        }
        Err(_) => {
            print_debug(&ext.print_tx, "net: ws::init_direct timed out").await;
            return Err(peer_rx);
        }
    }
}

pub async fn init_routed(
    ext: &IdentityExt,
    data: &NetData,
    peer_id: &Identity,
    router_id: &Identity,
    router_port: u16,
    peer_rx: mpsc::UnboundedReceiver<KernelMessage>,
) -> Result<(), mpsc::UnboundedReceiver<KernelMessage>> {
    match time::timeout(
        TIMEOUT,
        connect_with_handshake(ext, peer_id, router_port, Some(router_id), false),
    )
    .await
    {
        Ok(Ok(connection)) => {
            // maintain direct connection
            tokio::spawn(utils::maintain_connection(
                peer_id.name.clone(),
                data.peers.clone(),
                connection,
                peer_rx,
                ext.kernel_message_tx.clone(),
                ext.print_tx.clone(),
            ));
            Ok(())
        }
        Ok(Err(e)) => {
            print_debug(&ext.print_tx, &format!("net: error getting routed: {e}")).await;
            Err(peer_rx)
        }
        Err(_) => {
            print_debug(&ext.print_tx, "net: timed out while getting routed").await;
            Err(peer_rx)
        }
    }
}

/// one of our routers has a pending passthrough for us: connect to them
/// and set up a connection that will be maintained like a normal one
pub async fn recv_via_router(
    ext: IdentityExt,
    data: NetData,
    peer_id: Identity,
    router_id: Identity,
) {
    let Some((ip, port)) = router_id.ws_routing() else {
        return;
    };
    let Ok(ws_url) = make_conn_url(&ext.our_ip, ip, port, WS_PROTOCOL) else {
        return;
    };
    let Ok((socket, _response)) = connect_async(ws_url).await else {
        return;
    };
    match connect_with_handshake_via_router(&ext, &peer_id, &router_id, socket).await {
        Ok(connection) => {
            // maintain direct connection
            let (mut peer, peer_rx) = Peer::new(peer_id.clone(), false);
            peer.handle = Some(tokio::spawn(utils::maintain_connection(
                peer_id.name.clone(),
                data.peers.clone(),
                connection,
                peer_rx,
                ext.kernel_message_tx,
                ext.print_tx,
            )));
            data.peers.insert(peer_id.name, peer).await;
        }
        Err(e) => {
            print_debug(&ext.print_tx, &format!("net: error getting routed: {e}")).await;
        }
    }
}

async fn recv_connection(
    ext: IdentityExt,
    data: NetData,
    mut socket: WebSocket,
) -> anyhow::Result<()> {
    // before we begin XX handshake pattern, check first message over socket
    let first_message = &utils::recv(&mut socket).await?;

    // if the first message contains a "routing request",
    // we see if the target is someone we are actively routing for,
    // and create a Passthrough connection if so.
    // a Noise 'e' message with have len 32
    if first_message.len() != 32 {
        let (from_id, target_id) =
            validate_routing_request(&ext.our.name, first_message, &data.pki)?;
        return create_passthrough(
            &ext,
            from_id,
            target_id,
            &data,
            PendingStream::WebSocket(socket),
        )
        .await;
    }

    let (mut noise, our_static_key) = build_responder();
    let mut buf = vec![0u8; 65535];

    // <- e
    noise.read_message(first_message, &mut buf)?;

    // -> e, ee, s, es
    utils::send_protocol_handshake(
        &ext,
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
        .ok_or(anyhow!("unknown KNS name '{}'", their_handshake.name))?;
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        &their_id,
    )?;

    // if we already have a connection to this peer, kill it so we
    // don't build a duplicate connection
    if let Some(mut peer) = data.peers.get_mut(&their_handshake.name) {
        peer.kill();
    }

    let (mut peer, peer_rx) = Peer::new(their_id.clone(), their_handshake.proxy_request);
    peer.handle = Some(tokio::spawn(utils::maintain_connection(
        their_handshake.name,
        data.peers.clone(),
        PeerConnection {
            noise: noise.into_transport_mode()?,
            buf,
            socket,
        },
        peer_rx,
        ext.kernel_message_tx,
        ext.print_tx,
    )));
    data.peers.insert(their_id.name.clone(), peer).await;
    Ok(())
}

async fn connect_with_handshake(
    ext: &IdentityExt,
    peer_id: &Identity,
    port: u16,
    use_router: Option<&Identity>,
    proxy_request: bool,
) -> anyhow::Result<PeerConnection> {
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_initiator();

    let ip = match use_router {
        None => peer_id
            .get_ip()
            .ok_or(anyhow!("target has no IP address"))?,
        Some(router_id) => router_id
            .get_ip()
            .ok_or(anyhow!("router has no IP address"))?,
    };
    let ws_url = make_conn_url(&ext.our_ip, ip, &port, WS_PROTOCOL)?;
    let Ok((mut socket, _response)) = connect_async(ws_url).await else {
        return Err(anyhow!("failed to connect to target"));
    };

    // if this is a routed request, before starting XX handshake pattern, send a
    // routing request message over socket
    if use_router.is_some() {
        socket
            .send(tungstenite::Message::binary(rmp_serde::to_vec(
                &RoutingRequest {
                    protocol_version: 1,
                    source: ext.our.name.clone(),
                    signature: ext
                        .keypair
                        .sign(
                            [&peer_id.name, use_router.unwrap().name.as_str()]
                                .concat()
                                .as_bytes(),
                        )
                        .as_ref()
                        .to_vec(),
                    target: peer_id.name.clone(),
                },
            )?))
            .await?;
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
        &ext,
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

async fn connect_with_handshake_via_router(
    ext: &IdentityExt,
    peer_id: &Identity,
    router_id: &Identity,
    mut socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> anyhow::Result<PeerConnection> {
    // before beginning XX handshake pattern, send a routing request
    socket
        .send(tungstenite::Message::binary(rmp_serde::to_vec(
            &RoutingRequest {
                protocol_version: 1,
                source: ext.our.name.clone(),
                signature: ext
                    .keypair
                    .sign(
                        [peer_id.name.as_str(), router_id.name.as_str()]
                            .concat()
                            .as_bytes(),
                    )
                    .as_ref()
                    .to_vec(),
                target: peer_id.name.to_string(),
            },
        )?))
        .await?;

    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_responder();

    // <- e
    noise.read_message(&utils::recv(&mut socket).await?, &mut buf)?;

    // -> e, ee, s, es
    utils::send_protocol_handshake(
        ext,
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
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        &peer_id,
    )?;

    Ok(PeerConnection {
        noise: noise.into_transport_mode()?,
        buf,
        socket,
    })
}
