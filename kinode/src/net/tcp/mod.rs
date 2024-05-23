use crate::net::{
    types::{IdentityExt, NetData, TCP_PROTOCOL},
    utils::{
        build_responder, print_debug, print_loud, validate_handshake, validate_routing_request,
        validate_signature,
    },
};
use lib::types::core::{Identity, KernelMessage, NodeId, NodeRouting};
use {
    dashmap::DashMap,
    futures::{SinkExt, StreamExt},
    rand::seq::SliceRandom,
    ring::signature::Ed25519KeyPair,
    std::{collections::HashMap, sync::Arc},
    tokio::net::{TcpListener, TcpStream},
    tokio::{sync::mpsc, time},
};

mod utils;

/// only used in connection initialization
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub async fn receiver(ext: IdentityExt, data: NetData) -> anyhow::Result<()> {
    let tcp_port = ext.our.get_protocol_port(TCP_PROTOCOL).unwrap();
    let tcp = match TcpListener::bind(format!("0.0.0.0:{tcp_port}")).await {
        Ok(tcp) => tcp,
        Err(_e) => {
            return Err(anyhow::anyhow!(
                "net: fatal error: can't listen on port {tcp_port}, update your KNS identity or free up that port"
            ));
        }
    };

    print_debug(&ext.print_tx, &format!("net: listening on port {tcp_port}")).await;

    loop {
        match tcp.accept().await {
            Err(e) => {
                print_debug(
                    &ext.print_tx,
                    &format!("net: error accepting TCP connection: {e}"),
                )
                .await;
            }
            Ok((stream, socket_addr)) => {
                print_debug(
                    &ext.print_tx,
                    &format!("net: got TCP connection from {socket_addr}"),
                )
                .await;
                let ext = ext.clone();
                let data = data.clone();
                tokio::spawn(async move {
                    match time::timeout(TIMEOUT, recv_connection(ext.clone(), data, stream)).await {
                        Ok(Ok(())) => return,
                        Ok(Err(e)) => {
                            print_debug(
                                &ext.print_tx,
                                &format!("net: error receiving TCP connection: {e}"),
                            )
                            .await
                        }
                        Err(_e) => {
                            print_debug(
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

pub async fn recv_via_router(
    ext: IdentityExt,
    data: NetData,
    peer_id: Identity,
    router_id: Identity,
) {
    todo!()
}

async fn recv_connection(
    ext: IdentityExt,
    data: NetData,
    mut stream: TcpStream,
) -> anyhow::Result<()> {
    todo!()
}
