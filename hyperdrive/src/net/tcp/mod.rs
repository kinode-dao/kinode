use crate::net::{
    types::{IdentityExt, NetData, Peer, PendingStream, RoutingRequest, TCP_PROTOCOL},
    utils::{
        build_initiator, build_responder, create_passthrough, make_conn_url, print_debug,
        validate_handshake, validate_routing_request, TIMEOUT,
    },
};
use lib::types::core::{Identity, KernelMessage};
use {
    anyhow::anyhow,
    tokio::net::{TcpListener, TcpStream},
    tokio::{sync::mpsc, time},
};

pub mod utils;

pub struct PeerConnection {
    pub noise: snow::TransportState,
    pub buf: [u8; 65535],
    pub stream: TcpStream,
}

pub async fn receiver(ext: IdentityExt, data: NetData) -> anyhow::Result<()> {
    let tcp_port = ext
        .our
        .get_protocol_port(TCP_PROTOCOL)
        .expect("tcp port not found");
    let tcp = match TcpListener::bind(format!("0.0.0.0:{tcp_port}")).await {
        Ok(tcp) => tcp,
        Err(_e) => {
            return Err(anyhow::anyhow!(
                "net: fatal error: can't listen on port {tcp_port}, update your HNS identity or free up that port"
            ));
        }
    };

    print_debug(&ext.print_tx, &format!("net: listening on port {tcp_port}")).await;

    loop {
        match tcp.accept().await {
            Err(e) => {
                print_debug(
                    &ext.print_tx,
                    &format!("net: error accepting TCP connection: {e}"),
                )
                .await;
            }
            Ok((stream, socket_addr)) => {
                print_debug(
                    &ext.print_tx,
                    &format!("net: got TCP connection from {socket_addr}"),
                )
                .await;
                let ext = ext.clone();
                let data = data.clone();
                tokio::spawn(async move {
                    match time::timeout(TIMEOUT, recv_connection(ext.clone(), data, stream)).await {
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
                &format!("net: error in tcp::init_direct: {e}"),
            )
            .await;
            return Err(peer_rx);
        }
        Err(_) => {
            print_debug(&ext.print_tx, "net: tcp::init_direct timed out").await;
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

async fn recv_connection(
    ext: IdentityExt,
    data: NetData,
    mut stream: TcpStream,
) -> anyhow::Result<()> {
    // before we begin XX handshake pattern, check first message over socket
    let (len, first_message) = utils::recv_raw(&mut stream).await?;

    // if the first message contains a "routing request",
    // we see if the target is someone we are actively routing for,
    // and create a Passthrough connection if so.
    // a Noise 'e' message with have len 32
    if len != 32 {
        let (from_id, target_id) =
            validate_routing_request(&ext.our.name, &first_message, &data.pki)?;
        return create_passthrough(&ext, from_id, target_id, &data, PendingStream::Tcp(stream))
            .await;
    }

    let mut buf = [0u8; 65535];
    let (mut noise, our_static_key) = build_responder();

    // <- e
    noise.read_message(&first_message, &mut buf)?;

    // -> e, ee, s, es
    utils::send_protocol_handshake(
        &ext,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut stream,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake = utils::recv_protocol_handshake(&mut noise, &mut buf, &mut stream).await?;

    // now validate this handshake payload against the HNS PKI
    let their_id = data
        .pki
        .get(&their_handshake.name)
        .ok_or(anyhow!("unknown HNS name '{}'", their_handshake.name))?;
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
            stream,
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
    let ip = match use_router {
        None => peer_id
            .get_ip()
            .ok_or(anyhow!("target has no IP address"))?,
        Some(router_id) => router_id
            .get_ip()
            .ok_or(anyhow!("router has no IP address"))?,
    };
    let tcp_url = make_conn_url(&ext.our_ip, ip, &port, TCP_PROTOCOL)?;
    let Ok(mut stream) = tokio::net::TcpStream::connect(tcp_url.to_string()).await else {
        return Err(anyhow!("failed to connect to {tcp_url}"));
    };

    // if this is a routed request, before starting XX handshake pattern, send a
    // routing request message over socket
    if use_router.is_some() {
        utils::send_raw(
            &mut stream,
            &rmp_serde::to_vec(&RoutingRequest {
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
            })?,
        )
        .await?;
    }

    let mut buf = [0u8; 65535];
    let (mut noise, our_static_key) = build_initiator();

    // -> e
    let len = noise.write_message(&[], &mut buf)?;
    utils::send_raw(&mut stream, &buf[..len]).await?;

    // <- e, ee, s, es
    let their_handshake = utils::recv_protocol_handshake(&mut noise, &mut buf, &mut stream).await?;

    // now validate this handshake payload against the HNS PKI
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
        &mut stream,
        proxy_request,
    )
    .await?;

    Ok(PeerConnection {
        noise: noise.into_transport_mode()?,
        buf,
        stream,
    })
}

pub async fn recv_via_router(
    ext: IdentityExt,
    data: NetData,
    peer_id: Identity,
    router_id: Identity,
) {
    let Some((ip, port)) = router_id.tcp_routing() else {
        return;
    };
    let Ok(tcp_url) = make_conn_url(&ext.our_ip, ip, port, TCP_PROTOCOL) else {
        return;
    };
    let Ok(stream) = tokio::net::TcpStream::connect(tcp_url.to_string()).await else {
        return;
    };
    match connect_with_handshake_via_router(&ext, &peer_id, &router_id, stream).await {
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

async fn connect_with_handshake_via_router(
    ext: &IdentityExt,
    peer_id: &Identity,
    router_id: &Identity,
    mut stream: TcpStream,
) -> anyhow::Result<PeerConnection> {
    // before beginning XX handshake pattern, send a routing request
    utils::send_raw(
        &mut stream,
        &rmp_serde::to_vec(&RoutingRequest {
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
        })?,
    )
    .await?;

    let mut buf = [0u8; 65535];
    let (mut noise, our_static_key) = build_responder();

    // <- e
    noise.read_message(&utils::recv_raw(&mut stream).await?.1, &mut buf)?;

    // -> e, ee, s, es
    utils::send_protocol_handshake(
        ext,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut stream,
        false,
    )
    .await?;

    // <- s, se
    let their_handshake = utils::recv_protocol_handshake(&mut noise, &mut buf, &mut stream).await?;

    // now validate this handshake payload against the HNS PKI
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
        stream,
    })
}
