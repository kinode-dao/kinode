use crate::net::{
    types::{HandshakePayload, Peers},
    utils::{make_conn_url, print_debug, print_loud},
    ws::{PeerConnection, PendingPassthroughs, MESSAGE_MAX_SIZE, TIMEOUT},
};
use lib::core::{
    Address, Identity, KernelMessage, Message, MessageSender, NetAction, NodeId, PrintSender,
    ProcessId, Request,
};
use {
    futures::{SinkExt, StreamExt},
    ring::signature::Ed25519KeyPair,
    tokio::sync::mpsc::UnboundedReceiver,
    tokio::time,
    tokio_tungstenite::{connect_async, tungstenite, MaybeTlsStream, WebSocketStream},
};

/// should always be spawned on its own task
pub async fn maintain_connection(
    peer_name: NodeId,
    peers: Peers,
    mut conn: PeerConnection,
    mut peer_rx: UnboundedReceiver<KernelMessage>,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) {
    let mut last_message = std::time::Instant::now();
    loop {
        tokio::select! {
            recv_result = recv_protocol_message(&mut conn) => {
                match recv_result {
                    Ok(km) => {
                        if km.source.node != peer_name {
                            print_loud(
                                &print_tx,
                                &format!(
                                    "net: got message with spoofed source from {peer_name}!"
                                ),
                            ).await;
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
                        match send_protocol_message(&km, &mut conn).await {
                            Ok(()) => {
                                last_message = std::time::Instant::now();
                                continue
                            }
                            Err(e) => {
                                if e.to_string() == "message too large" {
                                    // this will result in a Timeout if the message
                                    // requested a response, otherwise nothing. so,
                                    // we should always print something to terminal
                                    print_loud(
                                        &print_tx,
                                        &format!(
                                            "net: tried to send too-large message, limit is {:.2}mb",
                                            MESSAGE_MAX_SIZE as f64 / 1_048_576.0
                                        ),
                                    ).await;
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
                match conn.socket.send(tungstenite::Message::Ping(vec![])).await {
                    Ok(()) => continue,
                    Err(_) => break,
                }
            }
            // if a message has not been sent or received in 2 hours, close the connection
            _ = tokio::time::sleep(std::time::Duration::from_secs(7200)) => {
                if last_message.elapsed().as_secs() > 7200 {
                    break
                }
            }
        }
    }
    let close_msg = match conn.socket.close(None).await {
        Ok(()) => format!("net: connection with {peer_name} closed"),
        Err(e) => format!("net: connection with {peer_name} closed: {e}"),
    };
    print_debug(&print_tx, &close_msg).await;
    peers.remove(&peer_name);
}

/// cross the streams
pub async fn maintain_passthrough(
    mut socket_1: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    mut socket_2: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) {
    let mut last_message = std::time::Instant::now();
    loop {
        tokio::select! {
            maybe_recv = socket_1.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        let Ok(()) = socket_2.send(msg).await else {
                            break
                        };
                        last_message = std::time::Instant::now();
                    }
                    _ => break,
                }
            },
            maybe_recv = socket_2.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        let Ok(()) = socket_1.send(msg).await else {
                            break
                        };
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
    let _ = socket_1.close(None).await;
    let _ = socket_2.close(None).await;
}

pub async fn create_passthrough(
    our: &Identity,
    our_ip: &str,
    from_id: Identity,
    target_id: Identity,
    peers: &Peers,
    pending_passthroughs: &mut PendingPassthroughs,
    socket_1: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> anyhow::Result<()> {
    // if the target has already generated a pending passthrough for this source,
    // immediately match them
    if let Some(((_to, _from), socket_2)) =
        pending_passthroughs.remove(&(target_id.name.clone(), from_id.name.clone()))
    {
        tokio::spawn(maintain_passthrough(socket_1, socket_2));
        return Ok(());
    }
    if let Some((ip, ws_port)) = target_id.ws_routing() {
        // create passthrough to direct node
        // TODO this won't ever happen currently since we validate
        // passthrough requests as being to a node we route for
        let ws_url = make_conn_url(our_ip, ip, ws_port, "ws")?;
        let Ok(Ok((socket_2, _response))) = time::timeout(TIMEOUT, connect_async(ws_url)).await
        else {
            return Err(anyhow::anyhow!("failed to connect to target"));
        };
        tokio::spawn(maintain_passthrough(socket_1, socket_2));
        return Ok(());
    }
    // create passthrough to indirect node that we do routing for
    //
    let target_peer = peers
        .get(&target_id.name)
        .ok_or(anyhow::anyhow!("can't route to that indirect node"))?;
    if !target_peer.routing_for {
        return Err(anyhow::anyhow!("we don't route for that indirect node"));
    }
    // send their net:distro:sys process a message, notifying it to create a *matching*
    // passthrough request, which we can pair with this pending one.
    target_peer.sender.send(KernelMessage {
        id: rand::random(),
        source: Address {
            node: our.name.clone(),
            process: ProcessId::new(Some("net"), "distro", "sys"),
        },
        target: Address {
            node: target_id.name.clone(),
            process: ProcessId::new(Some("net"), "distro", "sys"),
        },
        rsvp: None,
        message: Message::Request(Request {
            inherit: false,
            expects_response: Some(5),
            body: rmp_serde::to_vec(&NetAction::ConnectionRequest(from_id.name.clone()))?,
            metadata: None,
            capabilities: vec![],
        }),
        lazy_load_blob: None,
    })?;

    pending_passthroughs.insert((from_id.name, target_id.name), socket_1);
    Ok(())
}

pub async fn send_protocol_message(
    km: &KernelMessage,
    conn: &mut PeerConnection,
) -> anyhow::Result<()> {
    let serialized = rmp_serde::to_vec(km)?;
    if serialized.len() > MESSAGE_MAX_SIZE as usize {
        return Err(anyhow::anyhow!("message too large"));
    }

    let len = (serialized.len() as u32).to_be_bytes();
    let with_length_prefix = [len.to_vec(), serialized].concat();

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in with_length_prefix.chunks(65519) {
        let len = conn.noise.write_message(payload, &mut conn.buf)?;
        conn.socket
            .feed(tungstenite::Message::binary(&conn.buf[..len]))
            .await?;
    }
    conn.socket.flush().await?;
    Ok(())
}

/// any error in receiving a message will result in the connection being closed.
pub async fn recv_protocol_message(conn: &mut PeerConnection) -> anyhow::Result<KernelMessage> {
    let outer_len = conn
        .noise
        .read_message(&ws_recv(&mut conn.socket).await?, &mut conn.buf)?;

    if outer_len < 4 {
        return Err(anyhow::anyhow!("protocol message too small!"));
    }
    let length_bytes = [conn.buf[0], conn.buf[1], conn.buf[2], conn.buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);
    if msg_len > MESSAGE_MAX_SIZE {
        return Err(anyhow::anyhow!("message too large"));
    }

    let mut msg = Vec::with_capacity(msg_len as usize);
    msg.extend_from_slice(&conn.buf[4..outer_len]);

    while msg.len() < msg_len as usize {
        let len = conn
            .noise
            .read_message(&ws_recv(&mut conn.socket).await?, &mut conn.buf)?;
        msg.extend_from_slice(&conn.buf[..len]);
    }

    Ok(rmp_serde::from_slice(&msg)?)
}

pub async fn send_protocol_handshake(
    our: &Identity,
    keypair: &Ed25519KeyPair,
    noise_static_key: &[u8],
    noise: &mut snow::HandshakeState,
    buf: &mut [u8],
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    proxy_request: bool,
) -> anyhow::Result<()> {
    let our_hs = rmp_serde::to_vec(&HandshakePayload {
        protocol_version: 1,
        name: our.name.clone(),
        signature: keypair.sign(noise_static_key).as_ref().to_vec(),
        proxy_request,
    })
    .expect("failed to serialize handshake payload");

    let len = noise.write_message(&our_hs, buf)?;
    socket
        .send(tungstenite::Message::binary(&buf[..len]))
        .await?;
    Ok(())
}

pub async fn recv_protocol_handshake(
    noise: &mut snow::HandshakeState,
    buf: &mut [u8],
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> anyhow::Result<HandshakePayload> {
    let len = noise.read_message(&ws_recv(socket).await?, buf)?;
    Ok(rmp_serde::from_slice(&buf[..len])?)
}

/// Receive a byte array from a read stream. If this returns an error,
/// we should close the connection. Will automatically respond to 'PING' messages with a 'PONG'.
pub async fn ws_recv(
    socket: &mut WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> anyhow::Result<Vec<u8>> {
    loop {
        match socket.next().await {
            Some(Ok(tungstenite::Message::Ping(_))) => {
                socket.send(tungstenite::Message::Pong(vec![])).await?;
                continue;
            }
            Some(Ok(tungstenite::Message::Pong(_))) => continue,
            Some(Ok(tungstenite::Message::Binary(bin))) => return Ok(bin),
            _ => return Err(anyhow::anyhow!("websocket closed")),
        }
    }
}
