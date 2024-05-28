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

    let sock_ref = socket2::SockRef::from(&conn.stream);
    let mut ka = socket2::TcpKeepalive::new();
    ka = ka.with_time(std::time::Duration::from_secs(30));
    ka = ka.with_interval(std::time::Duration::from_secs(30));
    sock_ref
        .set_tcp_keepalive(&ka)
        .expect("failed to set tcp keepalive");

    loop {
        tokio::select! {
            maybe_recv = peer_rx.recv() => {
                let Some(km) = maybe_recv else {
                    break
                };
                let Ok(()) = send_protocol_message(&km, &mut conn).await else {
                    break
                };
            },
            outer_len = recv_protocol_message_init(&mut conn.stream) => {
                match outer_len {
                    Ok((read, outer_len)) => match recv_protocol_message(&mut conn, read, outer_len).await {
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
                    Err(e) => {
                        print_debug(&print_tx, &format!("net: error receiving message: {e}")).await;
                        break
                    }
                }
            },
        }
    }
    let _ = conn.stream.shutdown().await;
    print_debug(&print_tx, &format!("net: connection lost with {peer_name}")).await;
    peers.remove(&peer_name);
}

async fn send_protocol_message(
    km: &KernelMessage,
    conn: &mut PeerConnection,
) -> anyhow::Result<()> {
    println!(
        "initiatior: {}, sending_nonce: {}, receiving_nonce: {}\r",
        conn.noise.is_initiator(),
        conn.noise.sending_nonce(),
        conn.noise.receiving_nonce()
    );
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

async fn recv_protocol_message_init(stream: &mut TcpStream) -> anyhow::Result<(usize, [u8; 4])> {
    let mut outer_len = [0; 4];
    let read = stream.read(&mut outer_len).await?;
    Ok((read, outer_len))
}

/// any error in receiving a message will result in the connection being closed.
async fn recv_protocol_message(
    conn: &mut PeerConnection,
    already_read: usize,
    mut outer_len: [u8; 4],
) -> anyhow::Result<KernelMessage> {
    // fill out the rest of outer_len depending on how many bytes were read
    if already_read < 4 {
        conn.stream
            .read_exact(&mut outer_len[already_read..])
            .await?;
    }
    let outer_len = u32::from_be_bytes(outer_len) as usize;

    let mut msg = vec![0; outer_len];
    let mut ptr = 0;
    while ptr < outer_len {
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
    let mut len = [0; 2];
    stream.read_exact(&mut len).await?;
    let msg_len = u16::from_be_bytes(len);
    let mut msg = vec![0; msg_len as usize];
    stream.read_exact(&mut msg).await?;

    let len = noise.read_message(&msg, buf)?;
    Ok(rmp_serde::from_slice(&buf[..len])?)
}

/// make sure raw message is less than 65536 bytes
pub async fn send_raw(stream: &mut TcpStream, msg: &[u8]) -> anyhow::Result<()> {
    let len = (msg.len() as u16).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(msg).await?;
    Ok(stream.flush().await?)
}

/// make sure raw message is less than 65536 bytes
pub async fn recv_raw(stream: &mut TcpStream) -> anyhow::Result<(u16, Vec<u8>)> {
    let mut len = [0; 2];
    stream.read_exact(&mut len).await?;
    let msg_len = u16::from_be_bytes(len);

    let mut msg = vec![0; msg_len as usize];
    stream.read_exact(&mut msg).await?;
    Ok((msg_len, msg))
}
