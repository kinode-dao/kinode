use crate::net::{
    tcp::PeerConnection,
    types::{HandshakePayload, IdentityExt, Peers},
    utils::{print_debug, print_loud, IDLE_TIMEOUT, MESSAGE_MAX_SIZE},
};
use lib::types::core::{
    check_process_id_hypermap_safe, KernelMessage, MessageSender, NodeId, PrintSender,
};
use {
    tokio::io::{AsyncReadExt, AsyncWriteExt},
    tokio::net::{tcp::OwnedReadHalf, tcp::OwnedWriteHalf, TcpStream},
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
    let sock_ref = socket2::SockRef::from(&conn.stream);
    let mut ka = socket2::TcpKeepalive::new();
    ka = ka.with_time(std::time::Duration::from_secs(30));
    ka = ka.with_interval(std::time::Duration::from_secs(30));
    sock_ref
        .set_tcp_keepalive(&ka)
        .expect("failed to set tcp keepalive");

    let (mut read_stream, mut write_stream) = conn.stream.into_split();
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
    let write = async move {
        while let Some(km) = peer_rx.recv().await {
            let Ok(()) =
                send_protocol_message(&km, &mut our_cipher, write_buf, &mut write_stream).await
            else {
                break;
            };
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
    stream: &mut OwnedWriteHalf,
) -> anyhow::Result<()> {
    let serialized = rmp_serde::to_vec(km)?;
    if serialized.len() > MESSAGE_MAX_SIZE as usize {
        return Err(anyhow::anyhow!("message too large"));
    }

    let outer_len = (serialized.len() as u32).to_be_bytes();
    stream.write_all(&outer_len).await?;

    // 65519 = 65535 - 16 (TAGLEN)
    for payload in serialized.chunks(65519) {
        let len = cipher.encrypt(payload, buf)? as u16;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&buf[..len as usize]).await?;
    }
    Ok(stream.flush().await?)
}

/// any error in receiving a message will result in the connection being closed.
async fn recv_protocol_message(
    cipher: &mut snow::CipherState,
    buf: &mut [u8],
    stream: &mut OwnedReadHalf,
) -> anyhow::Result<KernelMessage> {
    stream.read_exact(&mut buf[..4]).await?;
    let outer_len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;

    let mut msg = vec![0; outer_len];
    let mut ptr = 0;
    while ptr < outer_len {
        let mut inner_len = [0; 2];
        stream.read_exact(&mut inner_len).await?;
        let inner_len = u16::from_be_bytes(inner_len);

        stream.read_exact(&mut buf[..inner_len as usize]).await?;
        let read_len = cipher.decrypt(&buf[..inner_len as usize], &mut msg[ptr..])?;
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
