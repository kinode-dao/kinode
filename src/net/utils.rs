use crate::net::{types::*, MESSAGE_MAX_SIZE, TIMEOUT};
use crate::types::*;
use anyhow::{anyhow, Result};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use ring::signature::{self, Ed25519KeyPair};
use snow::params::NoiseParams;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver},
    RwLock,
};
use tokio::task::JoinSet;
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
    peers: &mut Peers,
    peer_connections: &mut JoinSet<(String, Option<KernelMessage>)>,
    conn: PeerConnection,
    km: Option<KernelMessage>,
    kernel_message_tx: &MessageSender,
    print_tx: &PrintSender,
) -> Result<()> {
    println!("save_new_peer\r");
    let peer_name = identity.name.clone();
    let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
    if km.is_some() {
        peer_tx.send(km.unwrap())?
    }
    let peer = Arc::new(RwLock::new(Peer {
        identity: identity.clone(),
        routing_for,
        sender: peer_tx,
    }));
    peers.insert(peer_name, peer.clone());
    peer_connections.spawn(maintain_connection(
        peer,
        conn,
        peer_rx,
        kernel_message_tx.clone(),
        print_tx.clone(),
    ));
    Ok(())
}

pub async fn maintain_connection(
    peer: Arc<RwLock<Peer>>,
    mut conn: PeerConnection,
    mut peer_rx: UnboundedReceiver<KernelMessage>,
    kernel_message_tx: MessageSender,
    print_tx: PrintSender,
) -> (NodeId, Option<KernelMessage>) {
    println!("maintain_connection\r");
    let peer_name = peer.read().await.identity.name.clone();
    loop {
        tokio::select! {
            recv_result = recv_uqbar_message(&mut conn) => {
                match recv_result {
                    Ok(km) => {
                        if km.source.node != peer_name {
                            println!("net: got message with spoofed source: {}\r", km);
                            return (peer_name, None)
                        }
                        kernel_message_tx.send(km).await.expect("net error: fatal: kernel died");
                    }
                    Err(e) => {
                        println!("net: error receiving message: {}\r", e);
                        return (peer_name, None)
                    }
                }
            },
            maybe_recv = peer_rx.recv() => {
                match maybe_recv {
                    Some(km) => {
                        match send_uqbar_message(&km, &mut conn).await {
                            Ok(()) => continue,
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
                                    return (peer_name, None)
                                }
                                return (peer_name, Some(km))
                            }
                        }
                    }
                    None => {
                        println!("net: peer disconnected\r");
                        return (peer_name, None)
                    }
                }
            },
        }
    }
}

/// cross the streams
pub async fn maintain_passthrough(mut conn: PassthroughConnection) {
    println!("maintain_passthrough\r");
    loop {
        tokio::select! {
            maybe_recv = conn.read_stream_1.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        conn.write_stream_2.send(msg).await.expect("net error: fatal: kernel died");
                    }
                    _ => {
                        println!("net: passthrough broke\r");
                        return
                    }
                }
            },
            maybe_recv = conn.read_stream_2.next() => {
                match maybe_recv {
                    Some(Ok(msg)) => {
                        conn.write_stream_1.send(msg).await.expect("net error: fatal: kernel died");
                    }
                    _ => {
                        println!("net: passthrough broke\r");
                        return
                    }
                }
            },
        }
    }
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
    println!("create_passthrough\r");
    // if the target has already generated a pending passthrough for this source,
    // immediately match them
    println!("current: {:?}\r", pending_passthroughs.keys());
    println!("this one: {:?}\r", (to_name.clone(), from_id.name.clone()));
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
        if !target_peer.read().await.routing_for {
            return Err(anyhow!("we don't route for that indirect node"));
        }
        // send their net:sys:uqbar process a message, notifying it to create a *matching*
        // passthrough request, which we can pair with this pending one.
        target_peer.write().await.sender.send(KernelMessage {
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
    println!("validate_routing_request\r");
    let routing_request: RoutingRequest = rmp_serde::from_slice(buf)?;
    println!("routing request: {:?}\r", routing_request);
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
    println!("validate_handshake\r");
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

pub async fn recv_uqbar_message(conn: &mut PeerConnection) -> Result<KernelMessage> {
    let outer_len = conn
        .noise
        .read_message(&ws_recv(&mut conn.read_stream).await?, &mut conn.buf)?;
    if outer_len < 4 {
        return Err(anyhow!("uqbar message too small!"));
    }

    let length_bytes = [conn.buf[0], conn.buf[1], conn.buf[2], conn.buf[3]];
    let msg_len = u32::from_be_bytes(length_bytes);

    let mut msg = Vec::with_capacity(msg_len as usize);
    msg.extend_from_slice(&conn.buf[4..outer_len]);

    while msg.len() < msg_len as usize {
        let len = conn
            .noise
            .read_message(&ws_recv(&mut conn.read_stream).await?, &mut conn.buf)?;
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
    println!("send_uqbar_handshake\r");
    let our_hs = rmp_serde::to_vec(&HandshakePayload {
        name: our.name.clone(),
        signature: keypair.sign(noise_static_key).as_ref().to_vec(),
        protocol_version: 1,
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
) -> Result<HandshakePayload> {
    println!("recv_uqbar_handshake\r");
    let len = noise.read_message(&ws_recv(read_stream).await?, buf)?;

    // from buffer, read a sequence of bytes that deserializes to the
    // 1. QNS name of the sender
    // 2. a signature by their published networking key that signs the
    //    static key they will be using for this handshake
    // 3. the version number of the networking protocol (so we can upgrade it)
    Ok(rmp_serde::from_slice(&buf[..len])?)
}

pub async fn ws_recv(
    read_stream: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> Result<Vec<u8>> {
    let Some(Ok(tungstenite::Message::Binary(bin))) = read_stream.next().await else {
        return Err(anyhow!("websocket closed"));
    };
    Ok(bin)
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
