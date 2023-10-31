use crate::net2::{types::*, MESSAGE_MAX_SIZE, TIMEOUT};
use crate::types::*;
use anyhow::{anyhow, Result};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use ring::signature::{self, Ed25519KeyPair};
use snow::params::NoiseParams;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite, MaybeTlsStream, WebSocketStream};

lazy_static::lazy_static! {
    static ref PARAMS: NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
                                        .parse()
                                        .expect("net: couldn't build noise params?");
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
                ipc: serde_json::to_vec(&NetActions::ConnectionRequest(from_id.name.clone()))?,
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
    let Ok(ws_url) = make_ws_url(our_ip, ip, port) else {
        return Err(anyhow!("failed to parse websocket url"));
    };
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
    let routing_request: RoutingRequest = bincode::deserialize(buf)?;
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
    let serialized = bincode::serialize(km)?;
    if serialized.len() > MESSAGE_MAX_SIZE as usize {
        return Err(anyhow!("uqbar message too large"));
    }

    let len = (serialized.len() as u32).to_be_bytes();
    let with_length_prefix = [len.to_vec(), serialized].concat();

    for payload in with_length_prefix.chunks(65519) {
        // 65535 - 16 (TAGLEN)
        let len = conn.noise.write_message(payload, &mut conn.buf)?;
        ws_send(&mut conn.write_stream, &conn.buf[..len]).await?;
    }
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

    Ok(bincode::deserialize(&msg)?)
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
    let our_hs = bincode::serialize(&HandshakePayload {
        name: our.name.clone(),
        signature: keypair.sign(noise_static_key).as_ref().to_vec(),
        protocol_version: 1,
        proxy_request,
    })
    .expect("failed to serialize handshake payload");

    let len = noise.write_message(&our_hs, buf)?;
    ws_send(write_stream, &buf[..len]).await?;

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
    Ok(bincode::deserialize(&buf[..len])?)
}

pub async fn ws_recv(
    read_stream: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> Result<Vec<u8>> {
    let Some(Ok(tungstenite::Message::Binary(bin))) = read_stream.next().await else {
        return Err(anyhow!("websocket closed"));
    };
    Ok(bin)
}

pub async fn ws_send(
    write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    msg: &[u8],
) -> Result<()> {
    write_stream.send(tungstenite::Message::binary(msg)).await?;
    Ok(())
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

pub fn make_ws_url(our_ip: &str, ip: &str, port: &u16) -> Result<url::Url, SendErrorKind> {
    // if we have the same public IP as target, route locally,
    // otherwise they will appear offline due to loopback stuff
    let ip = if our_ip == ip { "localhost" } else { ip };
    match url::Url::parse(&format!("ws://{}:{}/ws", ip, port)) {
        Ok(v) => Ok(v),
        Err(_) => Err(SendErrorKind::Offline),
    }
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
