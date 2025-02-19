use crate::net::{
    types::{HandshakePayload, IdentityExt, Peers},
    utils::{print_debug, print_loud, IDLE_TIMEOUT, MESSAGE_MAX_SIZE},
    ws::{PeerConnection, WebSocket},
};
use lib::core::{check_process_id_hypermap_safe, KernelMessage, MessageSender, NodeId, PrintSender};
use {
    futures::{SinkExt, StreamExt},
    tokio::sync::mpsc::UnboundedReceiver,
    tokio_tungstenite::tungstenite,
};

type WsWriteHalf = futures::stream::SplitSink<WebSocket, tungstenite::Message>;
type WsReadHalf = futures::stream::SplitStream<WebSocket>;

/// should always be spawned on its own task
pub async fn maintain_connection(
    peer_name: NodeId,
    peers: Peers,
    mut conn: PeerConnection,
    mut peer_rx: UnboundedReceiver<KernelMessage>,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) {
    let (mut write_stream, mut read_stream) = conn.socket.split();
    let initiator = conn.noise.is_initiator();
    let snow::CipherStates(c1, c2) = conn.noise.extract_cipherstates();
    let (mut our_cipher, mut their_cipher) = if initiator {
        // if initiator, we write with first and read with second
        (c1, c2)
    } else {
        // if responder, we read with first and write with second
        (c2, c1)
    };

    let write_buf = &mut [0; 65536];
    let write_print_tx = print_tx.clone();
    let write = async move {
        loop {
            tokio::select! {
                Some(km) = peer_rx.recv() => {
                    if let Err(e) =
                        send_protocol_message(&km, &mut our_cipher, write_buf, &mut write_stream).await
                    {
                        if e.to_string() == "message too large" {
                            // this will result in a Timeout if the message
                            // requested a response, otherwise nothing. so,
                            // we should always print something to terminal
                            print_loud(
                                &write_print_tx,
                                &format!(
                                    "net: tried to send too-large message, limit is {:.2}mb",
                                    MESSAGE_MAX_SIZE as f64 / 1_048_576.0
                                ),
                            )
                            .await;
                        }
                        break;
                    }
                }
                // keepalive ping -- note that we don't look for pongs
                // just to close if the connection is truly dead
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                    match write_stream.send(tungstenite::Message::Ping(vec![])).await {
                        Ok(()) => continue,
                        Err(_) => break,
                    }
                }
            }
        }
    };

    let read_buf = &mut conn.buf;
    let read_peer_name = peer_name.clone();
    let read_print_tx = print_tx.clone();
    let read = async move {
        loop {
            match recv_protocol_message(&mut their_cipher, read_buf, &mut read_stream).await {
                Ok(km) => {
                    if km.source.node != read_peer_name {
                        print_loud(
                            &read_print_tx,
                            &format!("net: got message with spoofed source from {read_peer_name}!"),
                        )
                        .await;
                        break;
                    }
                    if check_process_id_hypermap_safe(&km.source.process).is_err() {
                        print_loud(
                            &read_print_tx,
                            &format!(
                                "net: got message from non-Hypermap-safe process: {}",
                                km.source
                            ),
                        )
                        .await;
                        break;
                    }
                    kernel_message_tx
                        .send(km)
                        .await
                        .expect("net: fatal: kernel receiver died");
                }
                Err(e) => {
                    print_debug(
                        &read_print_tx,
                        &format!("net: error receiving message: {e}"),
                    )
                    .await;
                    break;
                }
            }
        }
    };

    let timeout = tokio::time::sleep(IDLE_TIMEOUT);

    tokio::select! {
        _ = write => (),
        _ = read => (),
        _ = timeout => {
            print_debug(&print_tx, &format!("net: closing idle connection with {peer_name}")).await;
        }
    }

    print_debug(&print_tx, &format!("net: connection lost with {peer_name}")).await;
    peers.remove(&peer_name).await;
}

async fn send_protocol_message(
    km: &KernelMessage,
    cipher: &mut snow::CipherState,
    buf: &mut [u8],
    stream: &mut WsWriteHalf,
) -> anyhow::Result<()> {
    let serialized = rmp_serde::to_vec(km)?;
    if serialized.len() > MESSAGE_MAX_SIZE as usize {
        return Err(anyhow::anyhow!("message too large"));
    }

    let len = (serialized.len() as u32).to_be_bytes();
    let with_length_prefix = [len.to_vec(), serialized].concat();

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in with_length_prefix.chunks(65519) {
        let len = cipher.encrypt(payload, buf)?;
        stream
            .feed(tungstenite::Message::binary(&buf[..len]))
            .await?;
    }
    stream.flush().await?;
    Ok(())
}

/// any error in receiving a message will result in the connection being closed.
async fn recv_protocol_message(
    cipher: &mut snow::CipherState,
    buf: &mut [u8],
    stream: &mut WsReadHalf,
) -> anyhow::Result<KernelMessage> {
    let outer_len = cipher.decrypt(&recv_read_only(stream).await?, buf)?;

    if outer_len < 4 {
        return Err(anyhow::anyhow!("protocol message too small!"));
    }
    let length_bytes = [buf[0], buf[1], buf[2], buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);
    if msg_len > MESSAGE_MAX_SIZE {
        return Err(anyhow::anyhow!("message too large"));
    }

    // bad
    let mut msg = Vec::with_capacity(msg_len as usize);
    msg.extend_from_slice(&buf[4..outer_len]);

    while msg.len() < msg_len as usize {
        let len = cipher.decrypt(&recv_read_only(stream).await?, buf)?;
        msg.extend_from_slice(&buf[..len]);
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
/// we should close the connection.
///
/// Will automatically respond to 'PING' messages with a 'PONG'.
pub async fn recv(socket: &mut WebSocket) -> anyhow::Result<Vec<u8>> {
    loop {
        match socket.next().await {
            Some(Ok(tungstenite::Message::Ping(_))) => {
                socket.send(tungstenite::Message::Pong(vec![])).await?;
                continue;
            }
            Some(Ok(tungstenite::Message::Pong(_))) => continue,
            Some(Ok(tungstenite::Message::Binary(bin))) => return Ok(bin),
            _ => return Err(anyhow::anyhow!("invalid websocket message received")),
        }
    }
}

/// Receive a byte array from a read stream. If this returns an error,
/// we should close the connection.
pub async fn recv_read_only(socket: &mut WsReadHalf) -> anyhow::Result<Vec<u8>> {
    loop {
        match socket.next().await {
            Some(Ok(tungstenite::Message::Ping(_))) => continue,
            Some(Ok(tungstenite::Message::Pong(_))) => continue,
            Some(Ok(tungstenite::Message::Binary(bin))) => return Ok(bin),
            _ => return Err(anyhow::anyhow!("websocket closed")),
        }
    }
}
