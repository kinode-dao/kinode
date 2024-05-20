use crate::net::types::*;
use anyhow::{anyhow, Result};
use lib::types::core::*;
use ring::signature::{self};
use snow::params::NoiseParams;

lazy_static::lazy_static! {
    pub static ref PARAMS: NoiseParams = "Noise_XX_25519_ChaChaPoly_BLAKE2s"
                                        .parse()
                                        .expect("net: couldn't build noise params?");
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
    our_name: &str,
    buf: &[u8],
    pki: &OnchainPKI,
) -> Result<(Identity, NodeId)> {
    let routing_request: RoutingRequest = rmp_serde::from_slice(buf)?;
    let their_id = pki
        .get(&routing_request.source)
        .ok_or(anyhow!("unknown KNS name"))?;
    let their_networking_key = signature::UnparsedPublicKey::new(
        &signature::ED25519,
        net_key_string_to_hex(&their_id.networking_key),
    );
    their_networking_key
        .verify(
            [&routing_request.target, our_name].concat().as_bytes(),
            &routing_request.signature,
        )
        .map_err(|e| anyhow!("their_networking_key.verify failed: {:?}", e))?;
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
    if handshake.protocol_version != 1 {
        return Err(anyhow!("handshake protocol version mismatch"));
    }
    // verify their signature of their static key
    let their_networking_key = signature::UnparsedPublicKey::new(
        &signature::ED25519,
        net_key_string_to_hex(&their_id.networking_key),
    );
    their_networking_key
        .verify(their_static_key, &handshake.signature)
        .map_err(|e| anyhow!("their_networking_key.verify handshake failed: {:?}", e))?;
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

pub fn make_conn_url(our_ip: &str, ip: &str, port: &u16, protocol: &str) -> Result<url::Url> {
    // if we have the same public IP as target, route locally,
    // otherwise they will appear offline due to loopback stuff
    let ip = if our_ip == ip { "localhost" } else { ip };
    let url = url::Url::parse(&format!("{}://{}:{}", protocol, ip, port))?;
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
                lazy_load_blob: km.lazy_load_blob,
            },
        })
        .await?;
    Ok(())
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
) -> Result<()> {
    print_tx
        .send(Printout {
            verbosity: 0,
            content: format!(
                "\x1b[3;32m{}: {}\x1b[0m",
                km.source.node,
                std::str::from_utf8(body).unwrap_or("!!message parse error!!")
            ),
        })
        .await?;
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
        .await?;
    Ok(())
}

pub async fn print_debug(print_tx: &PrintSender, content: &str) {
    let _ = print_tx
        .send(Printout {
            verbosity: 2,
            content: content.into(),
        })
        .await;
}
