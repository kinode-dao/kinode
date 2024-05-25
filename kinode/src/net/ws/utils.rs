use crate::net::{
    types::{HandshakePayload, IdentityExt, Peers},
    utils::{print_debug, print_loud, MESSAGE_MAX_SIZE},
    ws::{PeerConnection, WebSocket},
};
use lib::core::{KernelMessage, MessageSender, NodeId, PrintSender};
use {
    futures::{SinkExt, StreamExt},
    tokio::sync::mpsc::UnboundedReceiver,
    tokio_tungstenite::tungstenite,
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
    println!("maintain_connection\r");
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

async fn send_protocol_message(
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
async fn recv_protocol_message(conn: &mut PeerConnection) -> anyhow::Result<KernelMessage> {
    let outer_len = conn
        .noise
        .read_message(&recv(&mut conn.socket).await?, &mut conn.buf)?;

    if outer_len < 4 {
        return Err(anyhow::anyhow!("protocol message too small!"));
    }
    let length_bytes = [conn.buf[0], conn.buf[1], conn.buf[2], conn.buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);
    if msg_len > MESSAGE_MAX_SIZE {
        return Err(anyhow::anyhow!("message too large"));
    }

    // bad
    let mut msg = Vec::with_capacity(msg_len as usize);
    msg.extend_from_slice(&conn.buf[4..outer_len]);

    while msg.len() < msg_len as usize {
        let len = conn
            .noise
            .read_message(&recv(&mut conn.socket).await?, &mut conn.buf)?;
        msg.extend_from_slice(&conn.buf[..len]);
    }

    Ok(rmp_serde::from_slice(&msg)?)
}

pub async fn send_protocol_handshake(
    ext: &IdentityExt,
    noise_static_key: &[u8],
    noise: &mut snow::HandshakeState,
    buf: &mut [u8],
    socket: &mut WebSocket,
    proxy_request: bool,
) -> anyhow::Result<()> {
    let our_hs = rmp_serde::to_vec(&HandshakePayload {
        protocol_version: 1,
        name: ext.our.name.clone(),
        signature: ext.keypair.sign(noise_static_key).as_ref().to_vec(),
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
    socket: &mut WebSocket,
) -> anyhow::Result<HandshakePayload> {
    let len = noise.read_message(&recv(socket).await?, buf)?;
    Ok(rmp_serde::from_slice(&buf[..len])?)
}

/// Receive a byte array from a read stream. If this returns an error,
/// we should close the connection. Will automatically respond to 'PING' messages with a 'PONG'.
pub async fn recv(socket: &mut WebSocket) -> anyhow::Result<Vec<u8>> {
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
