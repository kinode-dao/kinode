use crate::net::{
    tcp::PeerConnection,
    types::{HandshakePayload, IdentityExt, Peers},
    utils::print_loud,
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
                            kernel_message_tx.send(km).await.expect("net error: fatal: kernel receiver died");
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
                            Err(_e) => break,
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

    let len = (serialized.len() as u32).to_be_bytes();
    let with_length_prefix = [len.to_vec(), serialized].concat();

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in with_length_prefix.chunks(65519) {
        let len = conn.noise.write_message(payload, &mut conn.buf)?;
        conn.stream.write(&conn.buf[..len]).await?;
    }
    conn.stream.flush().await?;
    Ok(())
}

/// any error in receiving a message will result in the connection being closed.
pub async fn recv_protocol_message(conn: &mut PeerConnection) -> anyhow::Result<KernelMessage> {
    let mut len = [0u8; 4];
    conn.stream.read_exact(&mut len).await?;
    let outer_len = conn.noise.read_message(&len, &mut conn.buf)?;

    if outer_len < 4 {
        return Err(anyhow::anyhow!("protocol message too small!"));
    }
    let length_bytes = [conn.buf[0], conn.buf[1], conn.buf[2], conn.buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);

    let mut msg = Vec::with_capacity(msg_len as usize);

    while msg.len() < msg_len as usize {
        let ptr = msg.len();
        conn.stream.read(&mut conn.buf).await?;
        conn.noise.read_message(&conn.buf, &mut msg[ptr..])?;
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

    let len = (our_hs.len() as u32).to_be_bytes();
    let with_length_prefix = [len.to_vec(), our_hs].concat();

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in with_length_prefix.chunks(65519) {
        let len = noise.write_message(payload, buf)?;
        stream.write(&buf[..len]).await?;
    }
    stream.flush().await?;
    Ok(())
}

pub async fn recv_protocol_handshake(
    noise: &mut snow::HandshakeState,
    buf: &mut [u8],
    stream: &mut TcpStream,
) -> anyhow::Result<HandshakePayload> {
    println!("tcp_recv_protocol_handshake\r");
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).await?;
    let outer_len = noise.read_message(&len, buf)?;

    if outer_len < 4 {
        return Err(anyhow::anyhow!("protocol message too small!"));
    }
    let length_bytes = [buf[0], buf[1], buf[2], buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);

    let mut msg = Vec::with_capacity(msg_len as usize);

    while msg.len() < msg_len as usize {
        let ptr = msg.len();
        stream.read(buf).await?;
        noise.read_message(&buf, &mut msg[ptr..])?;
    }

    Ok(rmp_serde::from_slice(&msg)?)
}

pub async fn send(stream: &mut TcpStream, msg: Vec<u8>) -> anyhow::Result<()> {
    println!("tcp_send\r");
    let len = (msg.len() as u32).to_be_bytes();
    println!("tcp_send: msg_len: {}\r", msg.len());
    let with_length_prefix = [len.to_vec(), msg.to_vec()].concat();
    stream.write(&with_length_prefix).await?;
    stream.flush().await?;
    println!("tcp_send: sent\r");
    Ok(())
}

pub async fn recv(stream: &mut TcpStream) -> anyhow::Result<(u32, Vec<u8>)> {
    println!("tcp_recv\r");
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).await?;
    let msg_len = u32::from_be_bytes(len);
    println!("tcp_recv: msg_len: {}\r", msg_len);

    let mut msg = Vec::with_capacity(msg_len as usize);
    stream.read_exact(&mut msg).await?;
    Ok((msg_len, msg))
}
