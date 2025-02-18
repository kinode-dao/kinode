use crate::net::types::{IdentityExt, NetData, Peer};
use crate::net::{connect, tcp, utils, ws};
use lib::types::core::{Identity, NodeRouting};
use tokio::time;

pub async fn maintain_routers(ext: IdentityExt, data: NetData) -> anyhow::Result<()> {
    let NodeRouting::Routers(ref routers) = ext.our.routing else {
        return Err(anyhow::anyhow!("net: no routers to maintain"));
    };
    loop {
        for router_name in routers {
            if data.peers.contains_key(router_name.as_str()) {
                // already connected to this router
                continue;
            }
            let Some(router_id) = data.pki.get(router_name.as_str()) else {
                // router does not exist in PKI that we know of
                continue;
            };
            connect_to_router(&router_id, &ext, &data).await;
        }
        time::sleep(time::Duration::from_secs(4)).await;
    }
}

pub async fn connect_to_router(router_id: &Identity, ext: &IdentityExt, data: &NetData) {
    utils::print_debug(
        &ext.print_tx,
        &format!("net: attempting to connect to router {}", router_id.name),
    )
    .await;
    let (peer, peer_rx) = Peer::new(router_id.clone(), false);
    data.peers.insert(router_id.name.clone(), peer).await;
    if let Some((_ip, port)) = router_id.tcp_routing() {
        match tcp::init_direct(ext, data, &router_id, *port, true, peer_rx).await {
            Ok(()) => {
                utils::print_debug(
                    &ext.print_tx,
                    &format!("net: connected to router {} via tcp", router_id.name),
                )
                .await;
                return;
            }
            Err(peer_rx) => {
                return connect::handle_failed_connection(ext, data, router_id, peer_rx).await;
            }
        }
    }
    if let Some((_ip, port)) = router_id.ws_routing() {
        match ws::init_direct(ext, data, &router_id, *port, true, peer_rx).await {
            Ok(()) => {
                utils::print_debug(
                    &ext.print_tx,
                    &format!("net: connected to router {} via ws", router_id.name),
                )
                .await;
                return;
            }
            Err(peer_rx) => {
                return connect::handle_failed_connection(ext, data, router_id, peer_rx).await;
            }
        }
    }
}
