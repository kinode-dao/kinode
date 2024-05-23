use crate::net::types::{HandshakePayload, OnchainPKI, PKINames, PendingStream, RoutingRequest};
use lib::types::core::{
    Address, Identity, KernelMessage, KnsUpdate, Message, MessageSender, NetworkErrorSender,
    NodeRouting, PrintSender, Printout, ProcessId, Response, SendError, SendErrorKind,
    WrappedSendError,
};
use ring::signature::{self};
use snow::params::NoiseParams;

lazy_static::lazy_static! {
    pub static ref PARAMS: NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
                                        .parse()
                                        .expect("net: couldn't build noise params?");
}

/// cross the streams -- spawn on own task
pub async fn maintain_passthrough(socket_1: PendingStream, socket_2: PendingStream) {
    use tokio::io::copy;
    // copy from ws_socket to tcp_socket and vice versa
    // do not use bidirectional because if one side closes,
    // we want to close the entire passthrough
    match (socket_1, socket_2) {
        (PendingStream::Tcp(socket_1), PendingStream::Tcp(socket_2)) => {
            let (mut r1, mut w1) = tokio::io::split(socket_1);
            let (mut r2, mut w2) = tokio::io::split(socket_2);
            let c1 = copy(&mut r1, &mut w2);
            let c2 = copy(&mut r2, &mut w1);
            tokio::select! {
                _ = c1 => return,
                _ = c2 => return,
            }
        }
        (PendingStream::WebSocket(mut ws_socket), PendingStream::Tcp(tcp_socket))
        | (PendingStream::Tcp(tcp_socket), PendingStream::WebSocket(mut ws_socket)) => {
            let (mut r1, mut w1) = tokio::io::split(ws_socket.get_mut());
            let (mut r2, mut w2) = tokio::io::split(tcp_socket);
            let c1 = copy(&mut r1, &mut w2);
            let c2 = copy(&mut r2, &mut w1);
            tokio::select! {
                _ = c1 => return,
                _ = c2 => return,
            }
        }
        (PendingStream::WebSocket(mut socket_1), PendingStream::WebSocket(mut socket_2)) => {
            let (mut r1, mut w1) = tokio::io::split(socket_1.get_mut());
            let (mut r2, mut w2) = tokio::io::split(socket_2.get_mut());
            let c1 = copy(&mut r1, &mut w2);
            let c2 = copy(&mut r2, &mut w1);
            tokio::select! {
                _ = c1 => return,
                _ = c2 => return,
            }
        }
    }
}

pub fn ingest_log(log: KnsUpdate, pki: &OnchainPKI, names: &PKINames) {
    pki.insert(
        log.name.clone(),
        Identity {
            name: log.name.clone(),
            networking_key: log.public_key,
            routing: if log.ips.is_empty() {
                NodeRouting::Routers(log.routers)
            } else {
                NodeRouting::Direct {
                    ip: log.ips[0].clone(),
                    ports: log.ports,
                }
            },
        },
    );
    names.insert(log.node, log.name);
}

pub fn validate_signature(from: &str, signature: &[u8], message: &[u8], pki: &OnchainPKI) -> bool {
    if let Some(peer_id) = pki.get(from) {
        let their_networking_key = signature::UnparsedPublicKey::new(
            &signature::ED25519,
            net_key_string_to_hex(&peer_id.networking_key),
        );
        their_networking_key.verify(message, signature).is_ok()
    } else {
        false
    }
}

pub fn validate_routing_request(
    our_name: &String,
    buf: &[u8],
    pki: &OnchainPKI,
) -> anyhow::Result<(Identity, Identity)> {
    let routing_request: RoutingRequest = rmp_serde::from_slice(buf)?;
    let from_id = pki
        .get(&routing_request.source)
        .ok_or(anyhow::anyhow!("unknown KNS name"))?;
    let their_networking_key = signature::UnparsedPublicKey::new(
        &signature::ED25519,
        net_key_string_to_hex(&from_id.networking_key),
    );
    their_networking_key
        .verify(
            format!("{}{}", routing_request.target, our_name).as_bytes(),
            &routing_request.signature,
        )
        .map_err(|e| anyhow::anyhow!("their_networking_key.verify failed: {:?}", e))?;
    let target_id = pki
        .get(&routing_request.target)
        .ok_or(anyhow::anyhow!("unknown KNS name"))?;
    match target_id.routers() {
        Some(routers) => {
            if !routers.contains(our_name) {
                return Err(anyhow::anyhow!("not routing for them"));
            }
        }
        None => return Err(anyhow::anyhow!("not routing for them")),
    }
    if routing_request.target == routing_request.source {
        return Err(anyhow::anyhow!("can't route to self"));
    }
    Ok((from_id.clone(), target_id.clone()))
}

pub fn validate_handshake(
    handshake: &HandshakePayload,
    their_static_key: &[u8],
    their_id: &Identity,
) -> anyhow::Result<()> {
    if handshake.protocol_version != 1 {
        return Err(anyhow::anyhow!("handshake protocol version mismatch"));
    }
    // verify their signature of their static key
    let their_networking_key = signature::UnparsedPublicKey::new(
        &signature::ED25519,
        net_key_string_to_hex(&their_id.networking_key),
    );
    their_networking_key
        .verify(their_static_key, &handshake.signature)
        .map_err(|e| anyhow::anyhow!("their_networking_key.verify handshake failed: {:?}", e))?;
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
            .expect("net: couldn't build initiator?"),
        keypair.public,
    )
}

pub fn make_conn_url(
    our_ip: &str,
    ip: &str,
    port: &u16,
    protocol: &str,
) -> anyhow::Result<url::Url> {
    // if we have the same public IP as target, route locally,
    // otherwise they will appear offline due to loopback stuff
    let ip = if our_ip == ip { "localhost" } else { ip };
    let url = url::Url::parse(&format!("{}://{}:{}", protocol, ip, port))?;
    Ok(url)
}

pub async fn error_offline(km: KernelMessage, network_error_tx: &NetworkErrorSender) {
    network_error_tx
        .send(WrappedSendError {
            id: km.id,
            source: km.source,
            error: SendError {
                kind: SendErrorKind::Offline,
                target: km.target,
                message: km.message,
                lazy_load_blob: km.lazy_load_blob,
            },
        })
        .await
        .expect("net: network_error_tx was dropped");
}

pub fn net_key_string_to_hex(s: &str) -> Vec<u8> {
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default()
}

pub async fn parse_hello_message(
    our: &Identity,
    km: &KernelMessage,
    body: &[u8],
    kernel_message_tx: &MessageSender,
    print_tx: &PrintSender,
) {
    print_loud(
        print_tx,
        &format!(
            "\x1b[3;32m{}: {}\x1b[0m",
            km.source.node,
            std::str::from_utf8(body).unwrap_or("!!message parse error!!")
        ),
    )
    .await;
    kernel_message_tx
        .send(KernelMessage {
            id: km.id,
            source: Address {
                node: our.name.clone(),
                process: ProcessId::new(Some("net"), "distro", "sys"),
            },
            target: km.rsvp.as_ref().unwrap_or(&km.source).clone(),
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    body: "delivered".as_bytes().to_vec(),
                    metadata: None,
                    capabilities: vec![],
                },
                None,
            )),
            lazy_load_blob: None,
        })
        .await
        .expect("net: kernel_message_tx was dropped");
}

/// Create a terminal printout at verbosity level 0.
pub async fn print_loud(print_tx: &PrintSender, content: &str) {
    let _ = print_tx
        .send(Printout {
            verbosity: 0,
            content: content.into(),
        })
        .await;
}

/// Create a terminal printout at verbosity level 2.
pub async fn print_debug(print_tx: &PrintSender, content: &str) {
    let _ = print_tx
        .send(Printout {
            verbosity: 2,
            content: content.into(),
        })
        .await;
}
