use crate::net::{
    types::{IdentityExt, NetData, TCP_PROTOCOL},
    utils as net_utils,
};
use lib::types::core::{Identity, KernelMessage, NodeId, NodeRouting};
use {
    dashmap::DashMap,
    futures::{SinkExt, StreamExt},
    rand::seq::SliceRandom,
    ring::signature::Ed25519KeyPair,
    std::{collections::HashMap, sync::Arc},
    tokio::net::{TcpListener, TcpStream},
    tokio::task::JoinSet,
    tokio::{sync::mpsc, time},
    tokio_tungstenite::{
        accept_async, connect_async, tungstenite, MaybeTlsStream, WebSocketStream,
    },
};

mod utils;

/// only used in connection initialization
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn receiver(ext: IdentityExt, net_data: NetData) -> anyhow::Result<()> {
    let tcp_port = ext.our.get_protocol_port(TCP_PROTOCOL).unwrap();
    let tcp = match TcpListener::bind(format!("0.0.0.0:{tcp_port}")).await {
        Ok(tcp) => tcp,
        Err(_e) => {
            return Err(anyhow::anyhow!(
                "net: fatal error: can't listen on port {tcp_port}, update your KNS identity or free up that port"
            ));
        }
    };

    net_utils::print_debug(&ext.print_tx, &format!("net: listening on port {tcp_port}")).await;

    loop {
        match tcp.accept().await {
            Err(e) => {
                net_utils::print_debug(
                    &ext.print_tx,
                    &format!("net: error accepting TCP connection: {e}"),
                )
                .await;
            }
            Ok((stream, socket_addr)) => {
                net_utils::print_debug(
                    &ext.print_tx,
                    &format!("net: got TCP connection from {socket_addr}"),
                )
                .await;
                let ext = ext.clone();
                let net_data = net_data.clone();
                tokio::spawn(async move {
                    match time::timeout(TIMEOUT, recv_connection(ext.clone(), net_data, stream))
                        .await
                    {
                        Ok(Ok(())) => return,
                        Ok(Err(e)) => {
                            net_utils::print_debug(
                                &ext.print_tx,
                                &format!("net: error receiving TCP connection: {e}"),
                            )
                            .await
                        }
                        Err(_e) => {
                            net_utils::print_debug(
                                &ext.print_tx,
                                &format!("net: TCP connection from {socket_addr} timed out"),
                            )
                            .await
                        }
                    }
                });
            }
        }
    }
}

pub async fn init_direct(
    ext: &IdentityExt,
    data: &NetData,
    peer_id: &Identity,
    port: u16,
    proxy_request: bool,
    peer_rx: mpsc::UnboundedReceiver<KernelMessage>,
) -> Result<(), mpsc::UnboundedReceiver<KernelMessage>> {
    todo!()
}

pub async fn init_routed(
    ext: &IdentityExt,
    data: &NetData,
    peer_id: &Identity,
    router_id: &Identity,
    port: u16,
    peer_rx: mpsc::UnboundedReceiver<KernelMessage>,
) -> Result<(), mpsc::UnboundedReceiver<KernelMessage>> {
    todo!()
}

async fn recv_connection(
    ext: IdentityExt,
    net_data: NetData,
    mut stream: TcpStream,
) -> anyhow::Result<()> {
    // before we begin XX handshake pattern, check first message over socket
    let first_message = &utils::recv(&mut stream).await?;

    let mut buf = vec![0u8; 65535];
    let (mut noise, our_static_key) = net_utils::build_responder();
    let (mut read_stream, mut write_stream) = stream.split();

    // if the first message contains a "routing request",
    // we see if the target is someone we are actively routing for,
    // and create a Passthrough connection if so.
    // a Noise 'e' message with have len 32
    if first_message.len() != 32 {
        todo!();
    }

    Ok(())
}
