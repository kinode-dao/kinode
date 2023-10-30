use crate::types::*;
use anyhow::{anyhow, Result};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use ring::signature::{self, Ed25519KeyPair};
use serde::{Deserialize, Serialize};
use snow::params::NoiseParams;
use std::{collections::HashMap, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use tokio::time::timeout;
use tokio_tungstenite::{
    accept_async, connect_async, tungstenite, MaybeTlsStream, WebSocketStream,
};

lazy_static::lazy_static! {
    static ref PARAMS: NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
                                        .parse()
                                        .expect("net: couldn't build noise params?");
}

// only used in connection initialization, otherwise, nacks and Responses are only used for "timeouts"
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

const MESSAGE_MAX_SIZE: u32 = 104_858_000; // 100 MB -- TODO analyze as desired, apps can always chunk data into many messages

#[derive(Clone, Debug, Serialize, Deserialize)]
enum NetActions {
    QnsUpdate(QnsUpdate),
    QnsBatchUpdate(Vec<QnsUpdate>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct QnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub routers: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct HandshakePayload {
    pub name: NodeId,
    pub signature: Vec<u8>,
    pub protocol_version: u8,
}

struct OpenConnection {
    pub noise: snow::TransportState,
    pub buf: Vec<u8>,
    pub write_stream: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    pub read_stream: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
}

type Peers = HashMap<String, Arc<Peer>>;
type PKINames = HashMap<String, NodeId>; // TODO maybe U256 to String
type OnchainPKI = HashMap<String, Identity>;

struct Peer {
    pub identity: Identity,
    pub sender: UnboundedSender<KernelMessage>,
}

/// Entry point from the main kernel task. Runs forever, spawns listener and sender tasks.
pub async fn networking(
    our: Identity,
    our_ip: String,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    message_rx: MessageReceiver,
) -> Result<()> {
    println!("networking\r");
    println!("our identity: {:#?}\r", our);
    let our = Arc::new(our);
    // branch on whether we are a direct or indirect node
    match &our.ws_routing {
        None => {
            // indirect node: run the indirect networking strategy
            todo!("TODO implement indirect networking strategy")
        }
        Some((ip, port)) => {
            // direct node: run the direct networking strategy
            if &our_ip != ip {
                return Err(anyhow!(
                    "net: fatal error: IP address mismatch: {} != {}, update your QNS identity",
                    our_ip,
                    ip
                ));
            }
            let tcp = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
                Ok(tcp) => tcp,
                Err(_e) => {
                    return Err(anyhow!(
                        "net: fatal error: can't listen on port {}, update your QNS identity or free up that port",
                        port,
                    ));
                }
            };
            direct_networking(
                our.clone(),
                our_ip,
                tcp,
                keypair,
                kernel_message_tx,
                network_error_tx,
                print_tx,
                self_message_tx,
                message_rx,
            )
            .await
        }
    }
}

async fn direct_networking(
    our: Arc<Identity>,
    our_ip: String,
    tcp: TcpListener,
    keypair: Arc<Ed25519KeyPair>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    print_tx: PrintSender,
    self_message_tx: MessageSender,
    mut message_rx: MessageReceiver,
) -> Result<()> {
    println!("direct_networking\r");
    let mut pki: OnchainPKI = HashMap::new();
    let mut peers: Peers = HashMap::new();
    // mapping from QNS namehash to username
    let mut names: PKINames = HashMap::new();

    let mut active_connections = JoinSet::<(String, Option<KernelMessage>)>::new();

    // 1. receive messages from kernel and send out over our connections
    // 2. receive incoming TCP connections
    // 3. deal with active connections that die by removing the associated peer
    loop {
        tokio::select! {
            Some(km) = message_rx.recv() => {
                // got a message from kernel to send out over the network
                let target = &km.target.node;
                // if the message is for us, it's either a protocol-level "hello" message,
                // or a debugging command issued from our terminal. handle it here:
                if target == &our.name {
                    match handle_local_message(
                        &our,
                        km,
                        &peers,
                        &mut pki,
                        &mut names,
                        &kernel_message_tx,
                        &print_tx,
                    )
                    .await {
                        Ok(()) => {},
                        Err(e) => {
                            print_tx.send(Printout {
                                verbosity: 0,
                                content: format!("net: error handling local message: {}", e)
                            }).await?;
                        }
                    }
                }
                // if the message is for a peer we currently have a connection with,
                // try to send it to them
                else if let Some(peer) = peers.get_mut(target) {
                    peer.sender.send(km)?;
                }
                else if let Some(peer_id) = pki.get(target) {
                    // if the message is for a *direct* peer we don't have a connection with,
                    // try to establish a connection with them
                    if peer_id.ws_routing.is_some() {
                        match init_connection(&our, &our_ip, peer_id, &keypair).await {
                            Ok((peer_name, conn)) => {
                                let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                                let peer = Arc::new(Peer {
                                    identity: peer_id.clone(),
                                    sender: peer_tx,
                                });
                                peers.insert(peer_name, peer.clone());
                                peer.sender.send(km)?;
                                active_connections.spawn(maintain_connection(
                                    peer,
                                    conn,
                                    peer_rx,
                                    kernel_message_tx.clone(),
                                ));
                            }
                            Err(e) => {
                                println!("net: error initializing connection: {}", e);
                                error_offline(km, &network_error_tx).await?;
                            }
                        }
                    }
                    // if the message is for an *indirect* peer we don't have a connection with,
                    // do some routing and shit
                    else {
                        todo!()
                    }
                }
                // peer cannot be found in PKI, throw an offline error
                else {
                    error_offline(km, &network_error_tx).await?;
                }
            }
            Ok((stream, _socket_addr)) = tcp.accept() => {
                // TODO we can perform some amount of validation here
                // to prevent some amount of potential DDoS attacks.
                // can also block based on socket_addr
                match accept_async(MaybeTlsStream::Plain(stream)).await {
                    Ok(websocket) => {
                        let (peer_name, conn) = recv_connection(&our, &pki, &keypair, websocket).await?;
                        let (peer_tx, peer_rx) = unbounded_channel::<KernelMessage>();
                        let peer = Arc::new(Peer {
                            identity: pki.get(&peer_name).ok_or(anyhow!("jej"))?.clone(),
                            sender: peer_tx,
                        });
                        peers.insert(peer_name, peer.clone());
                        println!("received incoming connection\r");
                        active_connections.spawn(maintain_connection(
                            peer,
                            conn,
                            peer_rx,
                            kernel_message_tx.clone(),
                        ));
                    }
                    // ignore connections we failed to accept...?
                    Err(_) => {}
                }
            }
            Some(Ok((dead_peer, maybe_resend))) = active_connections.join_next() => {
                peers.remove(&dead_peer);
                match maybe_resend {
                    None => {},
                    Some(km) => {
                        self_message_tx.send(km).await?;
                    }
                }
            }
        }
    }
}

async fn maintain_connection(
    peer: Arc<Peer>,
    mut conn: OpenConnection,
    mut peer_rx: UnboundedReceiver<KernelMessage>,
    kernel_message_tx: MessageSender,
    // network_error_tx: NetworkErrorSender,
    // print_tx: PrintSender,
) -> (String, Option<KernelMessage>) {
    println!("maintain_connection\r");
    loop {
        tokio::select! {
            recv_result = recv_uqbar_message(&mut conn) => {
                match recv_result {
                    Ok(km) => {
                        if km.source.node != peer.identity.name {
                            println!("net: got message with spoofed source\r");
                            return (peer.identity.name.clone(), None)
                        }
                        kernel_message_tx.send(km).await.expect("net error: fatal: kernel died");
                    }
                    Err(e) => {
                        println!("net: error receiving message: {}\r", e);
                        return (peer.identity.name.clone(), None)
                    }
                }
            },
            maybe_recv = peer_rx.recv() => {
                match maybe_recv {
                    Some(km) => {
                        // TODO error handle
                        match send_uqbar_message(&km, &mut conn).await {
                            Ok(()) => continue,
                            Err(e) => {
                                println!("net: error sending message: {}\r", e);
                                return (peer.identity.name.clone(), Some(km))
                            }
                        }
                    }
                    None => {
                        println!("net: peer disconnected\r");
                        return (peer.identity.name.clone(), None)
                    }
                }
            },
        }
    }
}

async fn recv_connection(
    our: &Identity,
    pki: &OnchainPKI,
    keypair: &Ed25519KeyPair,
    websocket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
) -> Result<(String, OpenConnection)> {
    println!("recv_connection\r");
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_responder();
    let (mut write_stream, mut read_stream) = websocket.split();

    // <- e
    noise.read_message(&ws_recv(&mut read_stream).await?, &mut buf)?;

    // -> e, ee, s, es
    send_uqbar_handshake(
        &our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
    )
    .await?;

    // <- s, se
    let their_handshake = recv_uqbar_handshake(&mut noise, &mut buf, &mut read_stream).await?;

    // now validate this handshake payload against the QNS PKI
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        pki.get(&their_handshake.name)
            .ok_or(anyhow!("unknown QNS name"))?,
    )?;

    // Transition the state machine into transport mode now that the handshake is complete.
    let noise = noise.into_transport_mode()?;
    println!("handshake complete, noise session received\r");

    Ok((
        their_handshake.name,
        OpenConnection {
            noise,
            buf,
            write_stream,
            read_stream,
        },
    ))
}

async fn init_connection(
    our: &Identity,
    our_ip: &str,
    peer_id: &Identity,
    keypair: &Ed25519KeyPair,
) -> Result<(String, OpenConnection)> {
    println!("init_connection\r");
    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = build_initiator();

    let Some((ref ip, ref port)) = peer_id.ws_routing else {
        return Err(anyhow!("target has no routing information"));
    };
    let Ok(ws_url) = make_ws_url(our_ip, ip, port) else {
        return Err(anyhow!("failed to parse websocket url"));
    };
    let Ok(Ok((websocket, _response))) = timeout(TIMEOUT, connect_async(ws_url)).await else {
        return Err(anyhow!("failed to connect to target"));
    };
    let (mut write_stream, mut read_stream) = websocket.split();

    // -> e
    let len = noise.write_message(&[], &mut buf)?;
    ws_send(&mut write_stream, &buf[..len]).await?;

    // <- e, ee, s, es
    let their_handshake = recv_uqbar_handshake(&mut noise, &mut buf, &mut read_stream).await?;

    // now validate this handshake payload against the QNS PKI
    validate_handshake(
        &their_handshake,
        noise
            .get_remote_static()
            .ok_or(anyhow!("noise error: missing remote pubkey"))?,
        peer_id,
    )?;

    // -> s, se
    send_uqbar_handshake(
        &our,
        keypair,
        &our_static_key,
        &mut noise,
        &mut buf,
        &mut write_stream,
    )
    .await?;

    let noise = noise.into_transport_mode()?;
    println!("handshake complete, noise session initiated\r");

    Ok((
        their_handshake.name,
        OpenConnection {
            noise,
            buf,
            write_stream,
            read_stream,
        },
    ))
}

fn validate_handshake(
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

async fn send_uqbar_message(km: &KernelMessage, conn: &mut OpenConnection) -> Result<()> {
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

async fn recv_uqbar_message(conn: &mut OpenConnection) -> Result<KernelMessage> {
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

async fn send_uqbar_handshake(
    our: &Identity,
    keypair: &Ed25519KeyPair,
    noise_static_key: &[u8],
    noise: &mut snow::HandshakeState,
    buf: &mut Vec<u8>,
    write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
) -> Result<()> {
    println!("send_uqbar_handshake\r");
    let our_hs = bincode::serialize(&HandshakePayload {
        name: our.name.clone(),
        signature: keypair.sign(noise_static_key).as_ref().to_vec(),
        protocol_version: 1,
    })
    .expect("failed to serialize handshake payload");

    let len = noise.write_message(&our_hs, buf)?;
    ws_send(write_stream, &buf[..len]).await?;

    Ok(())
}

async fn recv_uqbar_handshake(
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

async fn ws_recv(
    read_stream: &mut SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) -> Result<Vec<u8>> {
    let Some(Ok(tungstenite::Message::Binary(bin))) = read_stream.next().await else {
        return Err(anyhow!("websocket closed"));
    };
    Ok(bin)
}

async fn ws_send(
    write_stream: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::Message>,
    msg: &[u8],
) -> Result<()> {
    write_stream.send(tungstenite::Message::binary(msg)).await?;
    Ok(())
}

fn build_responder() -> (snow::HandshakeState, Vec<u8>) {
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

fn build_initiator() -> (snow::HandshakeState, Vec<u8>) {
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

fn make_ws_url(our_ip: &str, ip: &str, port: &u16) -> Result<url::Url, SendErrorKind> {
    // if we have the same public IP as target, route locally,
    // otherwise they will appear offline due to loopback stuff
    let ip = if our_ip == ip { "localhost" } else { ip };
    match url::Url::parse(&format!("ws://{}:{}/ws", ip, port)) {
        Ok(v) => Ok(v),
        Err(_) => Err(SendErrorKind::Offline),
    }
}

async fn error_offline(km: KernelMessage, network_error_tx: &NetworkErrorSender) -> Result<()> {
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

/// net module only handles incoming local requests, will never return a response
async fn handle_local_message(
    our: &Identity,
    km: KernelMessage,
    peers: &Peers,
    pki: &mut OnchainPKI,
    names: &mut PKINames,
    kernel_message_tx: &MessageSender,
    print_tx: &PrintSender,
) -> Result<()> {
    let ipc = match km.message {
        Message::Response(_) => return Ok(()),
        Message::Request(request) => request.ipc,
    };

    if km.source.node != our.name {
        // respond to a text message with a simple "delivered" response
        print_tx
            .send(Printout {
                verbosity: 0,
                content: format!(
                    "\x1b[3;32m{}: {}\x1b[0m",
                    km.source.node,
                    std::str::from_utf8(&ipc).unwrap_or("!!message parse error!!")
                ),
            })
            .await?;
        kernel_message_tx
            .send(KernelMessage {
                id: km.id,
                source: Address {
                    node: our.name.clone(),
                    process: ProcessId::from_str("net:sys:uqbar").unwrap(),
                },
                target: km.rsvp.unwrap_or(km.source),
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        ipc: "delivered".as_bytes().to_vec(),
                        metadata: None,
                    },
                    None,
                )),
                payload: None,
                signed_capabilities: None,
            })
            .await?;
        Ok(())
    } else {
        // available commands: "peers", "QnsUpdate" (see qns_indexer module)
        // first parse as raw string, then deserialize to NetActions object
        match std::str::from_utf8(&ipc) {
            Ok("peers") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("{:#?}", peers.keys()),
                    })
                    .await?;
            }
            Ok("pki") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("{:#?}", pki),
                    })
                    .await?;
            }
            Ok("names") => {
                print_tx
                    .send(Printout {
                        verbosity: 0,
                        content: format!("{:#?}", names),
                    })
                    .await?;
            }
            _ => {
                let Ok(act) = serde_json::from_slice::<NetActions>(&ipc) else {
                    print_tx
                        .send(Printout {
                            verbosity: 0,
                            content: "net: got unknown command".into(),
                        })
                        .await?;
                    return Ok(());
                };
                match act {
                    NetActions::QnsUpdate(log) => {
                        print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!("net: got QNS update for {}", log.name),
                            })
                            .await?;

                        pki.insert(
                            log.name.clone(),
                            Identity {
                                name: log.name.clone(),
                                networking_key: log.public_key,
                                ws_routing: if log.ip == "0.0.0.0".to_string() || log.port == 0 {
                                    None
                                } else {
                                    Some((log.ip, log.port))
                                },
                                allowed_routers: log.routers,
                            },
                        );
                        names.insert(log.node, log.name);
                    }
                    NetActions::QnsBatchUpdate(log_list) => {
                        print_tx
                            .send(Printout {
                                verbosity: 1,
                                content: format!(
                                    "net: got QNS update with {} peers",
                                    log_list.len()
                                ),
                            })
                            .await?;
                        for log in log_list {
                            pki.insert(
                                log.name.clone(),
                                Identity {
                                    name: log.name.clone(),
                                    networking_key: log.public_key,
                                    ws_routing: if log.ip == "0.0.0.0".to_string() || log.port == 0
                                    {
                                        None
                                    } else {
                                        Some((log.ip, log.port))
                                    },
                                    allowed_routers: log.routers,
                                },
                            );
                            names.insert(log.node, log.name);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
