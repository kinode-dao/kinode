use crate::net::{
    tcp::PeerConnection,
    types::{HandshakePayload, IdentityExt, Peers},
    utils::{print_debug, print_loud, MESSAGE_MAX_SIZE},
};
use lib::types::core::{KernelMessage, MessageSender, NodeId, PrintSender};
use {
    tokio::io::{AsyncReadExt, AsyncWriteExt},
    tokio::net::TcpStream,
    tokio::sync::mpsc::UnboundedReceiver,
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
    println!("tcp_maintain_connection\r");
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
                            kernel_message_tx.send(km).await.expect("net: fatal: kernel receiver died");
                            continue
                        }
                    }
                    Err(e) => {
                        print_debug(&print_tx, &format!("net: error receiving message: {e}")).await;
                        break
                    }
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
                                print_debug(&print_tx, &format!("net: error sending message: {e}")).await;
                                break
                            }
                        }
                    }
                    None => break
                }
            },
            // if a message has not been sent or received in 2 hours, close the connection
            _ = tokio::time::sleep(std::time::Duration::from_secs(7200)) => {
                if last_message.elapsed().as_secs() > 7200 {
                    break
                }
            }
        }
    }
    let _ = conn.stream.shutdown().await;
    peers.remove(&peer_name);
}

pub async fn send_protocol_message(
    km: &KernelMessage,
    conn: &mut PeerConnection,
) -> anyhow::Result<()> {
    let serialized = rmp_serde::to_vec(km)?;
    if serialized.len() > MESSAGE_MAX_SIZE as usize {
        return Err(anyhow::anyhow!("message too large"));
    }

    let outer_len = (serialized.len() as u32).to_be_bytes();
    conn.stream.write_all(&outer_len).await?;

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in serialized.chunks(65519) {
        let len = conn.noise.write_message(payload, &mut conn.buf)? as u16;
        conn.stream.write_all(&len.to_be_bytes()).await?;
        conn.stream.write_all(&conn.buf[..len as usize]).await?;
    }
    Ok(conn.stream.flush().await?)
}

/// any error in receiving a message will result in the connection being closed.
pub async fn recv_protocol_message(conn: &mut PeerConnection) -> anyhow::Result<KernelMessage> {
    let mut outer_len = [0; 4];
    conn.stream.read_exact(&mut outer_len).await?;
    let outer_len = u32::from_be_bytes(outer_len);

    let mut msg = vec![0; outer_len as usize];
    let mut ptr = 0;
    while ptr < outer_len as usize {
        let mut inner_len = [0; 2];
        conn.stream.read_exact(&mut inner_len).await?;
        let inner_len = u16::from_be_bytes(inner_len);
        conn.stream
            .read_exact(&mut conn.buf[..inner_len as usize])
            .await?;
        let read_len = conn
            .noise
            .read_message(&conn.buf[..inner_len as usize], &mut msg[ptr..])?;
        ptr += read_len;
    }
    Ok(rmp_serde::from_slice(&msg)?)
}

pub async fn send_protocol_handshake(
    ext: &IdentityExt,
    noise_static_key: &[u8],
    noise: &mut snow::HandshakeState,
    buf: &mut [u8],
    stream: &mut TcpStream,
    proxy_request: bool,
) -> anyhow::Result<()> {
    println!("tcp_send_protocol_handshake\r");
    let our_hs = rmp_serde::to_vec(&HandshakePayload {
        protocol_version: 1,
        name: ext.our.name.clone(),
        signature: ext.keypair.sign(noise_static_key).as_ref().to_vec(),
        proxy_request,
    })
    .expect("failed to serialize handshake payload");

    let len = noise.write_message(&our_hs, buf)?;
    let len_bytes = (len as u16).to_be_bytes();
    stream.write_all(&len_bytes).await?;
    stream.write_all(&buf[..len]).await?;
    Ok(stream.flush().await?)
}

pub async fn recv_protocol_handshake(
    noise: &mut snow::HandshakeState,
    buf: &mut [u8],
    stream: &mut TcpStream,
) -> anyhow::Result<HandshakePayload> {
    println!("tcp_recv_protocol_handshake\r");
    let mut len = [0; 2];
    stream.read_exact(&mut len).await?;
    let msg_len = u16::from_be_bytes(len);
    let mut msg = vec![0; msg_len as usize];
    stream.read_exact(&mut msg).await?;

    let len = noise.read_message(&msg, buf)?;
    Ok(rmp_serde::from_slice(&buf[..len])?)
}

/// make sure message is less than 65536 bytes
pub async fn send_raw(stream: &mut TcpStream, msg: &[u8]) -> anyhow::Result<()> {
    let len = (msg.len() as u16).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(msg).await?;
    Ok(stream.flush().await?)
}

/// make sure message is less than 65536 bytes
pub async fn recv_raw(stream: &mut TcpStream) -> anyhow::Result<(u16, Vec<u8>)> {
    let mut len = [0; 2];
    stream.read_exact(&mut len).await?;
    let msg_len = u16::from_be_bytes(len);

    let mut msg = vec![0; msg_len as usize];
    stream.read_exact(&mut msg).await?;
    Ok((msg_len, msg))
}
