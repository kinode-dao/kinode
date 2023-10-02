use crate::net::*;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XChaCha20Poly1305, XNonce,
};
use elliptic_curve::ecdh::SharedSecret;
use futures::{SinkExt, StreamExt};
use ring::signature::Ed25519KeyPair;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite;

pub async fn build_connection(
    our: Identity,
    keypair: Arc<Ed25519KeyPair>,
    pki: OnchainPKI,
    keys: PeerKeys,
    peers: Peers,
    websocket: WebSocket,
    kernel_message_tx: MessageSender,
    net_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
    with: Option<String>,
) -> (
    UnboundedSender<(NetworkMessage, Option<ErrorShuttle>)>,
    JoinHandle<Option<String>>,
) {
    // println!("building new connection\r");
    let (message_tx, message_rx) = unbounded_channel::<(NetworkMessage, Option<ErrorShuttle>)>();
    let handle = tokio::spawn(maintain_connection(
        our,
        with,
        keypair,
        pki,
        keys,
        peers,
        websocket,
        message_tx.clone(),
        message_rx,
        kernel_message_tx,
        net_message_tx,
        network_error_tx,
    ));
    return (message_tx, handle);
}

/// Keeps a connection alive and handles sending and receiving of NetworkMessages through it.
/// TODO add a keepalive PING/PONG system
/// TODO kill this after a certain amount of inactivity
pub async fn maintain_connection(
    our: Identity,
    with: Option<String>,
    keypair: Arc<Ed25519KeyPair>,
    pki: OnchainPKI,
    keys: PeerKeys,
    peers: Peers,
    websocket: WebSocket,
    message_tx: UnboundedSender<(NetworkMessage, Option<ErrorShuttle>)>,
    mut message_rx: UnboundedReceiver<(NetworkMessage, Option<ErrorShuttle>)>,
    kernel_message_tx: MessageSender,
    net_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
) -> Option<String> {
    // let conn_id: u64 = rand::random();
    // println!("maintaining connection {conn_id}\r");

    // accept messages on the websocket in one task, and send messages in another
    let (mut write_stream, mut read_stream) = websocket.split();

    let (forwarding_ack_tx, mut forwarding_ack_rx) = unbounded_channel::<MessageResult>();
    // manage outstanding ACKs from messages sent over the connection
    // TODO replace with more performant data structure
    let ack_map = Arc::new(RwLock::new(HashMap::<u64, ErrorShuttle>::new()));
    let sender_ack_map = ack_map.clone();

    let forwarder_message_tx = message_tx.clone();
    let ack_forwarder = tokio::spawn(async move {
        while let Some(result) = forwarding_ack_rx.recv().await {
            match result {
                Ok(NetworkMessage::Ack(id)) => {
                    // println!("net: got forwarding ack for message {}\r", id);
                    forwarder_message_tx
                        .send((NetworkMessage::Ack(id), None))
                        .unwrap();
                }
                Ok(NetworkMessage::Nack(id)) => {
                    // println!("net: got forwarding nack for message {}\r", id);
                    forwarder_message_tx
                        .send((NetworkMessage::Nack(id), None))
                        .unwrap();
                }
                Ok(NetworkMessage::HandshakeAck(handshake)) => {
                    // println!(
                    //     "net: got forwarding handshakeAck for message {}\r",
                    //     handshake.id
                    // );
                    forwarder_message_tx
                        .send((NetworkMessage::HandshakeAck(handshake), None))
                        .unwrap();
                }
                Err((message_id, _e)) => {
                    // println!("net: got forwarding error from ack_rx: {:?}\r", e);
                    // what do we do here?
                    forwarder_message_tx
                        .send((NetworkMessage::Nack(message_id), None))
                        .unwrap();
                }
                _ => {
                    // println!("net: weird none ack\r");
                }
            }
        }
    });

    // receive messages from over the websocket and route them to the correct peer handler,
    // or create it, if necessary.
    let ws_receiver = tokio::spawn(async move {
        while let Some(Ok(tungstenite::Message::Binary(bin))) = read_stream.next().await {
            // TODO use a language-netural serialization format here!
            let Ok(net_message) = bincode::deserialize::<NetworkMessage>(&bin) else {
                // just kill the connection if we get a non-Uqbar message
                break;
            };
            match net_message {
                NetworkMessage::Ack(id) => {
                    let Some(result_tx) = ack_map.write().await.remove(&id) else {
                        // println!("conn {conn_id}: got unexpected Ack {id}\r");
                        continue;
                    };
                    // println!("conn {conn_id}: got Ack {id}\r");
                    let _ = result_tx.send(Ok(net_message));
                    continue;
                }
                NetworkMessage::Nack(id) => {
                    let Some(result_tx) = ack_map.write().await.remove(&id) else {
                        // println!("net: got unexpected Nack\r");
                        continue;
                    };
                    let _ = result_tx.send(Ok(net_message));
                    continue;
                }
                NetworkMessage::Msg {
                    ref id,
                    ref from,
                    ref to,
                    ref contents,
                } => {
                    // println!("conn {conn_id}: handling msg {id}\r");
                    // if the message is *directed to us*, try to handle with the
                    // matching peer handler "decrypter".
                    //
                    if to == &our.name {
                        // if we have the peer, send the message to them.
                        if let Some(peer) = peers.read().await.get(from) {
                            let _ = peer
                                .decrypter
                                .send((contents.to_owned(), forwarding_ack_tx.clone()));
                            continue;
                        }
                        // if we don't have the peer, see if we have the keys to create them.
                        // if we don't have their keys, throw a nack.
                        if let Some((peer_id, secret)) = keys.read().await.get(from) {
                            let new_peer = create_new_peer(
                                our.clone(),
                                peer_id.clone(),
                                peers.clone(),
                                keys.clone(),
                                secret.clone(),
                                message_tx.clone(),
                                kernel_message_tx.clone(),
                                net_message_tx.clone(),
                                network_error_tx.clone(),
                            );
                            let _ = new_peer
                                .decrypter
                                .send((contents.to_owned(), forwarding_ack_tx.clone()));
                            peers.write().await.insert(peer_id.name.clone(), new_peer);
                        } else {
                            // println!("net: nacking message {id}\r");
                            message_tx.send((NetworkMessage::Nack(*id), None)).unwrap();
                        }
                    } else {
                        // if the message is *directed to someone else*, try to handle
                        // with the matching peer handler "sender".
                        //
                        if let Some(peer) = peers.read().await.get(to) {
                            let _ = peer.sender.send((
                                PeerMessage::Net(net_message),
                                Some(forwarding_ack_tx.clone()),
                            ));
                        } else {
                            // if we don't have the peer, throw a nack.
                            // println!("net: nacking message with id {id}\r");
                            message_tx.send((NetworkMessage::Nack(*id), None)).unwrap();
                        }
                    }
                }
                NetworkMessage::Handshake(ref handshake) => {
                    // when we get a handshake, if we are the target,
                    // 1. verify it against the PKI
                    // 2. send a response handshakeAck
                    // 3. create a Peer and save, replacing old one if it existed
                    // as long as we are the target, we also get to kill this connection
                    // if the handshake is invalid, since it must be directly "to" us.
                    if handshake.target == our.name {
                        let Some(peer_id) = pki.read().await.get(&handshake.from).cloned() else {
                            // println!(
                            //     "net: failed handshake with unknown node {}\r",
                            //     handshake.from
                            // );
                            message_tx
                                .send((NetworkMessage::Nack(handshake.id), None))
                                .unwrap();
                            break;
                        };
                        let their_ephemeral_pk = match validate_handshake(&handshake, &peer_id) {
                            Ok(pk) => pk,
                            Err(e) => {
                                println!("net: invalid handshake from {}: {}\r", handshake.from, e);
                                message_tx
                                    .send((NetworkMessage::Nack(handshake.id), None))
                                    .unwrap();
                                break;
                            }
                        };
                        let (secret, handshake) = make_secret_and_handshake(
                            &our,
                            keypair.clone(),
                            &handshake.from,
                            Some(handshake.id),
                        );
                        message_tx
                            .send((NetworkMessage::HandshakeAck(handshake), None))
                            .unwrap();
                        let secret = Arc::new(secret.diffie_hellman(&their_ephemeral_pk));
                        // save the handshake to our Keys map
                        keys.write()
                            .await
                            .insert(peer_id.name.clone(), (peer_id.clone(), secret.clone()));
                        let new_peer = create_new_peer(
                            our.clone(),
                            peer_id.clone(),
                            peers.clone(),
                            keys.clone(),
                            secret,
                            message_tx.clone(),
                            kernel_message_tx.clone(),
                            net_message_tx.clone(),
                            network_error_tx.clone(),
                        );
                        // we might be replacing an old peer, so we need to remove it first
                        // we can't rely on the hashmap for this, because the dropped peer
                        // will trigger a drop of the sender, which will kill the peer_handler
                        peers.write().await.remove(&peer_id.name);
                        peers.write().await.insert(peer_id.name.clone(), new_peer);
                    } else {
                        // if we are NOT the target,
                        // try to send it to the matching peer handler "sender"
                        if let Some(peer) = peers.read().await.get(&handshake.target) {
                            let _ = peer.sender.send((
                                PeerMessage::Net(net_message),
                                Some(forwarding_ack_tx.clone()),
                            ));
                        } else {
                            // if we don't have the peer, throw a nack.
                            // println!("net: nacking handshake with id {}\r", handshake.id);
                            message_tx
                                .send((NetworkMessage::Nack(handshake.id), None))
                                .unwrap();
                        }
                    }
                }
                NetworkMessage::HandshakeAck(ref handshake) => {
                    let Some(result_tx) = ack_map.write().await.remove(&handshake.id) else {
                        continue;
                    };
                    let _ = result_tx.send(Ok(net_message));
                }
            }
        }
    });

    tokio::select! {
        _ = ws_receiver => {
            // println!("ws_receiver died\r");
        },
        _ = ack_forwarder => {
            // println!("ack_forwarder died\r");
        }
        // receive messages we would like to send to peers along this connection
        // and send them to the websocket
        _ = async {
            while let Some((message, result_tx)) = message_rx.recv().await {
                // TODO use a language-netural serialization format here!
                if let Ok(bytes) = bincode::serialize::<NetworkMessage>(&message) {
                    match &message {
                        NetworkMessage::Msg { id, .. } => {
                            // println!("conn {conn_id}: piping msg {id}\r");
                            sender_ack_map.write().await.insert(*id, result_tx.unwrap());
                        }
                        NetworkMessage::Handshake(h) => {
                            sender_ack_map.write().await.insert(h.id, result_tx.unwrap());
                        }
                        _ => {}
                    }
                    match write_stream.send(tungstenite::Message::Binary(bytes)).await {
                        Ok(()) => {}
                        Err(e) => {
                            // println!("net: send error: {:?}\r", e);
                            let id = match &message {
                                NetworkMessage::Msg { id, .. } => id,
                                NetworkMessage::Handshake(h) => &h.id,
                                _ => continue,
                            };
                            let Some(result_tx) = sender_ack_map.write().await.remove(&id) else {
                                continue;
                            };
                            // TODO learn how to handle other non-fatal websocket errors.
                            match e {
                                tungstenite::error::Error::Capacity(_)
                                | tungstenite::Error::Io(_) => {
                                    let _ = result_tx.send(Err((*id, SendErrorKind::Timeout)));
                                }
                                _ => {
                                    let _ = result_tx.send(Ok(NetworkMessage::Nack(*id)));
                                }
                            }
                        }
                    }
                }
            }
        } => {
            // println!("ws_sender died\r");
        },
    };
    return with;
}

/// After a successful handshake, use information to spawn a new `peer_handler` task
/// and save a `Peer` in our peers mapping. Returns a sender to use for sending messages
/// to this peer, which will also be saved in its Peer struct.
pub fn create_new_peer(
    our: Identity,
    new_peer_id: Identity,
    peers: Peers,
    keys: PeerKeys,
    secret: Arc<SharedSecret<Secp256k1>>,
    conn_sender: UnboundedSender<(NetworkMessage, Option<ErrorShuttle>)>,
    kernel_message_tx: MessageSender,
    net_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
) -> Peer {
    let (message_tx, message_rx) = unbounded_channel::<(PeerMessage, Option<ErrorShuttle>)>();
    let (decrypter_tx, decrypter_rx) = unbounded_channel::<(Vec<u8>, ErrorShuttle)>();
    let peer_id_name = new_peer_id.name.clone();
    let peer_conn_sender = conn_sender.clone();
    tokio::spawn(async move {
        match peer_handler(
            our,
            peer_id_name.clone(),
            secret,
            message_rx,
            decrypter_rx,
            peer_conn_sender,
            kernel_message_tx,
            network_error_tx,
        )
        .await
        {
            None => {
                // println!("net: dropping peer handler but not deleting\r");
            }
            Some(km) => {
                // println!("net: ok actually deleting peer+keys now and retrying send\r");
                peers.write().await.remove(&peer_id_name);
                keys.write().await.remove(&peer_id_name);
                let _ = net_message_tx.send(km).await;
            }
        }
    });
    return Peer {
        identity: new_peer_id,
        sender: message_tx,
        decrypter: decrypter_tx,
        socket_tx: conn_sender,
    };
}

/// 1. take in messages from a specific peer, decrypt them, and send to kernel
/// 2. take in messages targeted at specific peer and either:
/// - encrypt them, and send to proper connection
/// - forward them untouched along the connection
async fn peer_handler(
    our: Identity,
    who: String,
    secret: Arc<SharedSecret<Secp256k1>>,
    mut message_rx: UnboundedReceiver<(PeerMessage, Option<ErrorShuttle>)>,
    mut decrypter_rx: UnboundedReceiver<(Vec<u8>, ErrorShuttle)>,
    socket_tx: UnboundedSender<(NetworkMessage, Option<ErrorShuttle>)>,
    kernel_message_tx: MessageSender,
    network_error_tx: NetworkErrorSender,
) -> Option<KernelMessage> {
    // println!("peer_handler\r");
    let mut key = [0u8; 32];
    secret
        .extract::<sha2::Sha256>(None)
        .expand(&[], &mut key)
        .unwrap();
    let cipher = XChaCha20Poly1305::new(generic_array::GenericArray::from_slice(&key));

    let (ack_tx, mut ack_rx) = unbounded_channel::<MessageResult>();
    // TODO use a more efficient data structure
    let ack_map = Arc::new(RwLock::new(HashMap::<u64, KernelMessage>::new()));
    let recv_ack_map = ack_map.clone();
    tokio::select! {
        //
        // take in messages from a specific peer, decrypt them, and send to kernel
        //
        _ = async {
            while let Some((encrypted_bytes, result_tx)) = decrypter_rx.recv().await {
                let nonce = XNonce::from_slice(&encrypted_bytes[..24]);
                if let Ok(decrypted) = cipher.decrypt(&nonce, &encrypted_bytes[24..]) {
                    if let Ok(message) = bincode::deserialize::<KernelMessage>(&decrypted) {
                        if message.source.node == who {
                            // println!("net: got peer message {}, acking\r", message.id);
                            let _ = result_tx.send(Ok(NetworkMessage::Ack(message.id)));
                            let _ = kernel_message_tx.send(message).await;
                            continue;
                        }
                        println!("net: got message 'from' wrong person! cheater/liar!\r");
                        break;
                    }
                    println!("net: failed to deserialize message from {}\r", who);
                    continue;
                }
                println!("net: failed to decrypt message from {}, could be spoofer\r", who);
                continue;
            }
        } => {
            // println!("net: lost peer {who}\r");
            return None
        }
        //
        // take in messages targeted at specific peer and either:
        // - encrypt them, and send to proper connection
        // - forward them untouched along the connection
        //
        _ = async {
            // if we get a result_tx, rather than track it here, let a different
            // part of the code handle whatever comes back from the socket.
            while let Some((message, maybe_result_tx)) = message_rx.recv().await {
                // if message is raw, we should encrypt.
                // otherwise, simply send
                match message {
                    PeerMessage::Raw(message) => {
                        let id = message.id;
                        if let Ok(bytes) = bincode::serialize::<KernelMessage>(&message) {
                            // generating a random nonce for each message.
                            // this isn't really as secure as we could get: should
                            // add a counter and then throw away the key when we hit a
                            // certain # of messages. TODO.
                            let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
                            if let Ok(encrypted) = cipher.encrypt(&nonce, bytes.as_ref()) {
                                if maybe_result_tx.is_none() {
                                    ack_map.write().await.insert(id, message);
                                }
                                match socket_tx.send((
                                    NetworkMessage::Msg {
                                        from: our.name.clone(),
                                        to: who.clone(),
                                        id: id,
                                        contents: [nonce.to_vec(), encrypted].concat(),
                                    },
                                    Some(maybe_result_tx.unwrap_or(ack_tx.clone())),
                                )) {
                                    Ok(()) => tokio::task::yield_now().await,
                                    Err(tokio::sync::mpsc::error::SendError((_, result_tx))) => {
                                        // println!("net: lost socket with {who}\r");
                                        let _ = result_tx.unwrap().send(Ok(NetworkMessage::Nack(id)));
                                    },
                                }
                            }
                        }
                    }
                    PeerMessage::Net(net_message) => {
                        match socket_tx.send((net_message, Some(maybe_result_tx.unwrap_or(ack_tx.clone())))) {
                            Ok(()) => continue,
                            Err(tokio::sync::mpsc::error::SendError((net_message, result_tx))) => {
                                // println!("net: lost *forwarding* socket with {who}\r");
                                let id = match net_message {
                                    NetworkMessage::Msg { id, .. } => id,
                                    NetworkMessage::Handshake(h) => h.id,
                                    _ => continue,
                                };
                                let _ = result_tx.unwrap().send(Ok(NetworkMessage::Nack(id)));
                                break;
                            },
                        }
                    }
                }
            }
        } => return None,
        //
        // receive acks and nacks from our socket
        // throw away acks, but kill this peer and retry the send on nacks.
        //
        maybe_km = async {
            while let Some(result) = ack_rx.recv().await {
                match result {
                    Ok(NetworkMessage::Ack(id)) => {
                        // println!("net: got ack for message {}\r", id);
                        recv_ack_map.write().await.remove(&id);
                        continue;
                    }
                    Ok(NetworkMessage::Nack(id)) => {
                        // println!("net: got nack for message {}\r", id);
                        let Some(km) = recv_ack_map.write().await.remove(&id) else {
                            continue;
                        };
                        // when we get a Nack, **delete this peer** and try to send the message again!
                        return Some(km)
                    }
                    Err((message_id, e)) => {
                        // println!("net: got error from ack_rx: {:?}\r", e);
                        // in practice this is always a timeout in current implementation
                        let Some(km) = recv_ack_map.write().await.remove(&message_id) else {
                            continue;
                        };
                        let _ = network_error_tx
                            .send(WrappedSendError {
                                id: km.id,
                                source: km.source,
                                error: SendError {
                                    kind: e,
                                    target: km.target,
                                    message: km.message,
                                    payload: km.payload,
                                },
                            })
                            .await;
                        return None
                    }
                    _ => {
                        // println!("net: weird none ack\r");
                        return None
                    }
                }
            }
            return None;
        } => {
            // println!("net: exiting peer due to nackage\r");
            return maybe_km
        },
    }
}
