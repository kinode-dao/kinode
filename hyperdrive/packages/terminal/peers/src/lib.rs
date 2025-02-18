use hyperware_process_lib::{net, script, Address, Message, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

script!(init);
fn init(_our: Address, _args: String) -> String {
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetPeers).unwrap())
        .send_and_await_response(10)
    else {
        return "Failed to get peers from networking module".to_string();
    };
    let Ok(net::NetResponse::Peers(identities)) = rmp_serde::from_slice(&body) else {
        return "Got malformed response from networking module".to_string();
    };
    let identities = identities
        .iter()
        .map(|peer_id| {
            format!(
                "{}:\n    networking key: {}\n    routing: {:?}",
                peer_id.name, peer_id.networking_key, peer_id.routing
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("identities of current connected peers:\n{identities}")
}
