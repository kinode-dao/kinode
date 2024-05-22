use crate::{
    net::{
        types::{IdentityExt, NetData},
        utils,
    },
    KNS_ADDRESS,
};
use lib::types::core::{Identity, KernelMessage, NodeId, NodeRouting};
use {
    anyhow::{anyhow, Result},
    dashmap::DashMap,
    futures::{SinkExt, StreamExt},
    rand::seq::SliceRandom,
    ring::signature::Ed25519KeyPair,
    std::{collections::HashMap, sync::Arc},
    tokio::net::TcpListener,
    tokio::task::JoinSet,
    tokio::{sync::mpsc, time},
    tokio_tungstenite::{
        accept_async, connect_async, tungstenite, MaybeTlsStream, WebSocketStream,
    },
};

/// only used in connection initialization, otherwise, nacks and Responses are only used for "timeouts"
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Entry point from the main kernel task. Runs forever, spawns listener and sender tasks.
pub async fn receiver(ext: IdentityExt, net_data: NetData) {
    todo!()
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
