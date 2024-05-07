use kinode_process_lib::{call_init, net, println, Address, Message, NodeId, Request};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
});

// types copied from runtime networking core

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub name: NodeId,
    pub networking_key: String,
    pub ws_routing: Option<(String, u16)>,
    pub allowed_routers: Vec<NodeId>,
}

call_init!(init);
fn init(_our: Address) {
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetPeers).unwrap())
        .send_and_await_response(5)
    else {
        println!("failed to get peers from networking module");
        return;
    };
    let Ok(net::NetResponse::Peers(identities)) = rmp_serde::from_slice(&body) else {
        println!("got malformed response from networking module");
        return;
    };
    let identities = identities
        .iter()
        .map(|peer_id| {
            format!(
                "{}:\n    networking key: {}\n    routing: {:?}\n    routers: {:?}",
                peer_id.name, peer_id.networking_key, peer_id.ws_routing, peer_id.allowed_routers
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    println!("identities of current connected peers:\n{identities}");
}
