use crate::net::{types::*, MESSAGE_MAX_SIZE, TIMEOUT};
use crate::types::*;
use anyhow::{anyhow, Result};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use ring::signature::{self, Ed25519KeyPair};
use snow::params::NoiseParams;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite, MaybeTlsStream, WebSocketStream};

lazy_static::lazy_static! {
    static ref PARAMS: NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
                                        .parse()
                                        .expect("net: couldn't build noise params?");
}

pub async fn save_new_peer(
    identity: &Identity,
    routing_for: bool,
    peers: Peers,
    conn: PeerConnection,
    km: Option<KernelMessage>,
    kernel_message_tx: &MessageSender,
    print_tx: &PrintSender,
) {
    print_debug(
        &print_tx,
        &format!("net: saving new peer {}", identity.name),
    )
    .await;
    let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
    if km.is_some() {
        peer_tx.send(km.unwrap()).unwrap()
    }
    let peer = Peer {
        identity: identity.clone(),
        routing_for,
        sender: peer_tx,
    };
    peers.insert(identity.name.clone(), peer.clone());
    tokio::spawn(maintain_connection(
        peer,
        peers,
        conn,
        peer_rx,
        kernel_message_tx.clone(),
        print_tx.clone(),
    ));
}

/// should always be spawned on its own task
pub async fn maintain_connection(
    peer: Peer,
    peers: Peers,
    mut conn: PeerConnection,
    mut peer_rx: UnboundedReceiver<KernelMessage>,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) {
    let peer_name = peer.identity.name;
    let mut last_message = std::time::Instant::now();
    loop {
        tokio::select! {
            recv_result = recv_uqbar_message(&mut conn) => {
                match recv_result {
                    Ok(km) => {
                        if km.source.node != peer_name {
                            let _ = print_tx.send(Printout {
                                verbosity: 0,
                                content: format!("net: got message with spoofed source from {peer_name}")
                            }).await;
                            break
                        } else {
                            kernel_message_tx.send(km).await.expect("net error: fatal: kernel receiver died");
                            last_message = std::time::Instant::now();
                            continue
                        }

                    }
                    Err(_) => break
                }
            },
            maybe_recv = peer_rx.recv() => {
                match maybe_recv {
                    Some(km) => {
                        match send_uqbar_message(&km, &mut conn).await {
                            Ok(()) => {
                                last_message = std::time::Instant::now();
                                continue
                            }
                            Err(e) => {
                                if e.to_string() == "message too large" {
                                    // this will result in a Timeout if the message
                                    // requested a response, otherwise nothing. so,
                                    // we should always print something to terminal
                                    let _ = print_tx.send(Printout {
                                        verbosity: 0,
                                        content: format!(
                                            "net: tried to send too-large message, limit is {:.2}mb",
                                            MESSAGE_MAX_SIZE as f64 / 1_048_576.0
                                        ),
                                    }).await;
                                }
                                break
                            }
                        }
                    }
                    None => break
                }
            },
            // keepalive ping -- can adjust time based on testing
            _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                match conn.write_stream.send(tungstenite::Message::Ping(vec![])).await {
                    Ok(()) => continue,
                    Err(_) => break,
                }
            }
            // if a message has not been sent or received in ~2 hours, close the connection
            _ = tokio::time::sleep(std::time::Duration::from_secs(7200)) => {
                if last_message.elapsed().as_secs() > 7200 {
                    break
                }
            }
        }
    }
    let mut conn = conn.write_stream.reunite(conn.read_stream).unwrap();
    let _ = conn.close(None).await;

    print_debug(&print_tx, &format!("net: connection with {peer_name} died")).await;
    peers.remove(&peer_name);
    return;
}

/// cross the streams
pub async fn maintain_passthrough(mut conn: PassthroughConnection) {
    let mut last_message = std::time::Instant::now();
    loop {
        tokio::select! {
            maybe_recv = conn.read_stream_1.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        conn.write_stream_2.send(msg).await.expect("net error: fatal: kernel died");
                        last_message = std::time::Instant::now();
                    }
                    _ => break,
                }
            },
            maybe_recv = conn.read_stream_2.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        conn.write_stream_1.send(msg).await.expect("net error: fatal: kernel died");
                        last_message = std::time::Instant::now();
                    }
                    _ => break,
                }
            },
            // if a message has not been sent or received in ~2 hours, close the connection
            _ = tokio::time::sleep(std::time::Duration::from_secs(7200)) => {
                if last_message.elapsed().as_secs() > 7200 {
                    break
                }
            }
        }
    }
    let mut conn_1 = conn.write_stream_1.reunite(conn.read_stream_1).unwrap();
    let mut conn_2 = conn.write_stream_2.reunite(conn.read_stream_2).unwrap();
    let _ = conn_1.close(None).await;
    let _ = conn_2.close(None).await;
}

pub async fn create_passthrough(
    our: &Identity,
    our_ip: &str,
    from_id: Identity,
    to_name: NodeId,
    pki: &OnchainPKI,
    peers: &Peers,
    pending_passthroughs: &mut PendingPassthroughs,
    write_stream_1: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    read_stream_1: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> Result<(Identity, Connection)> {
    // if the target has already generated a pending passthrough for this source,
    // immediately match them
    if let Some(pending) = pending_passthroughs.remove(&(to_name.clone(), from_id.name.clone())) {
        return Ok((
            from_id,
            Connection::Passthrough(PassthroughConnection {
                write_stream_1,
                read_stream_1,
                write_stream_2: pending.write_stream,
                read_stream_2: pending.read_stream,
            }),
        ));
    }
    let to_id = pki.get(&to_name).ok_or(anyhow!("unknown QNS name"))?;
    let Some((ref ip, ref port)) = to_id.ws_routing else {
        // create passthrough to indirect node that we do routing for
        //
        let target_peer = peers
            .get(&to_name)
            .ok_or(anyhow!("can't route to that indirect node"))?;
        // TODO: if we're not router for an indirect node, we should be able to
        // *use one of their routers* to create a doubly-indirect passthrough.
        if !target_peer.routing_for {
            return Err(anyhow!("we don't route for that indirect node"));
        }
        // send their net:sys:uqbar process a message, notifying it to create a *matching*
        // passthrough request, which we can pair with this pending one.
        target_peer.sender.send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our.name.clone(),
                process: ProcessId::from_str("net:sys:uqbar").unwrap(),
            },
            target: Address {
                node: to_name.clone(),
                process: ProcessId::from_str("net:sys:uqbar").unwrap(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                expects_response: Some(5),
                ipc: rmp_serde::to_vec(&NetActions::ConnectionRequest(from_id.name.clone()))?,
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        })?;

        return Ok((
            from_id,
            Connection::PendingPassthrough(PendingPassthroughConnection {
                target: to_name,
                write_stream: write_stream_1,
                read_stream: read_stream_1,
            }),
        ));
    };
    // create passthrough to direct node
    //
    let ws_url = make_ws_url(our_ip, ip, port)?;
    let Ok(Ok((websocket, _response))) = timeout(TIMEOUT, connect_async(ws_url)).await else {
        return Err(anyhow!("failed to connect to target"));
    };
    let (write_stream_2, read_stream_2) = websocket.split();
    Ok((
        from_id,
        Connection::Passthrough(PassthroughConnection {
            write_stream_1,
            read_stream_1,
            write_stream_2,
            read_stream_2,
        }),
    ))
}

pub fn validate_routing_request(
    our_name: &str,
    buf: &[u8],
    pki: &OnchainPKI,
) -> Result<(Identity, NodeId)> {
    let routing_request: RoutingRequest = rmp_serde::from_slice(buf)?;
    let their_id = pki
        .get(&routing_request.source)
        .ok_or(anyhow!("unknown QNS name"))?;
    let their_networking_key = signature::UnparsedPublicKey::new(
        &signature::ED25519,
        hex::decode(&strip_0x(&their_id.networking_key))?,
    );
    their_networking_key.verify(
        &[&routing_request.target, our_name].concat().as_bytes(),
        &routing_request.signature,
    )?;
    if routing_request.target == routing_request.source {
        return Err(anyhow!("can't route to self"));
    }
    Ok((their_id.clone(), routing_request.target))
}

pub fn validate_handshake(
    handshake: &HandshakePayload,
    their_static_key: &[u8],
    their_id: &Identity,
) -> Result<()> {
    if handshake.protocol_version != 1 {
        return Err(anyhow!("handshake protocol version mismatch"));
    }
    // verify their signature of their static key
    let their_networking_key = signature::UnparsedPublicKey::new(
        &signature::ED25519,
        hex::decode(&strip_0x(&their_id.networking_key))?,
    );
    their_networking_key.verify(their_static_key, &handshake.signature)?;
    Ok(())
}

pub async fn send_uqbar_message(km: &KernelMessage, conn: &mut PeerConnection) -> Result<()> {
    let serialized = rmp_serde::to_vec(km)?;
    if serialized.len() > MESSAGE_MAX_SIZE as usize {
        return Err(anyhow!("message too large"));
    }

    let len = (serialized.len() as u32).to_be_bytes();
    let with_length_prefix = [len.to_vec(), serialized].concat();

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in with_length_prefix.chunks(65519) {
        let len = conn.noise.write_message(payload, &mut conn.buf)?;
        conn.write_stream
            .feed(tungstenite::Message::binary(&conn.buf[..len]))
            .await?;
    }
    conn.write_stream.flush().await?;
    Ok(())
}

/// any error in receiving a message will result in the connection being closed.
pub async fn recv_uqbar_message(conn: &mut PeerConnection) -> Result<KernelMessage> {
    let outer_len = conn.noise.read_message(
        &ws_recv(&mut conn.read_stream, &mut conn.write_stream).await?,
        &mut conn.buf,
    )?;
    if outer_len < 4 {
        return Err(anyhow!("uqbar message too small!"));
    }

    let length_bytes = [conn.buf[0], conn.buf[1], conn.buf[2], conn.buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);
    if msg_len > MESSAGE_MAX_SIZE {
        return Err(anyhow!("message too large"));
    }

    let mut msg = Vec::with_capacity(msg_len as usize);
    msg.extend_from_slice(&conn.buf[4..outer_len]);

    while msg.len() < msg_len as usize {
        let len = conn.noise.read_message(
            &ws_recv(&mut conn.read_stream, &mut conn.write_stream).await?,
            &mut conn.buf,
        )?;
        msg.extend_from_slice(&conn.buf[..len]);
    }

    Ok(rmp_serde::from_slice(&msg)?)
}

pub async fn send_uqbar_handshake(
    our: &Identity,
    keypair: &Ed25519KeyPair,
    noise_static_key: &[u8],
    noise: &mut snow::HandshakeState,
    buf: &mut Vec<u8>,
    write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    proxy_request: bool,
) -> Result<()> {
    let our_hs = rmp_serde::to_vec(&HandshakePayload {
        protocol_version: 1,
        name: our.name.clone(),
        signature: keypair.sign(noise_static_key).as_ref().to_vec(),
        proxy_request,
    })
    .expect("failed to serialize handshake payload");

    let len = noise.write_message(&our_hs, buf)?;
    write_stream
        .send(tungstenite::Message::binary(&buf[..len]))
        .await?;
    Ok(())
}

pub async fn recv_uqbar_handshake(
    noise: &mut snow::HandshakeState,
    buf: &mut Vec<u8>,
    read_stream: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
) -> Result<HandshakePayload> {
    let len = noise.read_message(&ws_recv(read_stream, write_stream).await?, buf)?;
    Ok(rmp_serde::from_slice(&buf[..len])?)
}

/// Receive a byte array from a read stream. If this returns an error,
/// we should close the connection. Will automatically respond to 'PING' messages with a 'PONG'.
pub async fn ws_recv(
    read_stream: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
) -> Result<Vec<u8>> {
    loop {
        match read_stream.next().await {
            Some(Ok(tungstenite::Message::Ping(_))) => {
                write_stream
                    .send(tungstenite::Message::Pong(vec![]))
                    .await?;
                continue;
            }
            Some(Ok(tungstenite::Message::Pong(_))) => continue,
            Some(Ok(tungstenite::Message::Binary(bin))) => return Ok(bin),
            _ => return Err(anyhow!("websocket closed")),
        }
    }
}

pub fn build_responder() -> (snow::HandshakeState, Vec<u8>) {
    let builder: snow::Builder<'_> = snow::Builder::new(PARAMS.clone());
    let keypair = builder
        .generate_keypair()
        .expect("net: couldn't generate keypair?");
    (
        builder
            .local_private_key(&keypair.private)
            .build_responder()
            .expect("net: couldn't build responder?"),
        keypair.public,
    )
}

pub fn build_initiator() -> (snow::HandshakeState, Vec<u8>) {
    let builder: snow::Builder<'_> = snow::Builder::new(PARAMS.clone());
    let keypair = builder
        .generate_keypair()
        .expect("net: couldn't generate keypair?");
    (
        builder
            .local_private_key(&keypair.private)
            .build_initiator()
            .expect("net: couldn't build responder?"),
        keypair.public,
    )
}

pub fn make_ws_url(our_ip: &str, ip: &str, port: &u16) -> Result<url::Url> {
    // if we have the same public IP as target, route locally,
    // otherwise they will appear offline due to loopback stuff
    let ip = if our_ip == ip { "localhost" } else { ip };
    let url = url::Url::parse(&format!("ws://{}:{}/ws", ip, port))?;
    Ok(url)
}

pub async fn error_offline(km: KernelMessage, network_error_tx: &NetworkErrorSender) -> Result<()> {
    network_error_tx
        .send(WrappedSendError {
            id: km.id,
            source: km.source,
            error: SendError {
                kind: SendErrorKind::Offline,
                target: km.target,
                message: km.message,
                payload: km.payload,
            },
        })
        .await?;
    Ok(())
}

fn strip_0x(s: &str) -> String {
    if s.starts_with("0x") {
        s[2..].to_string()
    } else {
        s.to_string()
    }
}

pub async fn print_debug(print_tx: &PrintSender, content: &str) {
    let _ = print_tx
        .send(Printout {
            verbosity: 1,
            content: content.into(),
        })
        .await;
}
