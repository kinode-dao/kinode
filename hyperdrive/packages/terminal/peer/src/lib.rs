use hyperware_process_lib::{net, script, Address, Message, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

script!(init);
fn init(_our: Address, args: String) -> String {
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetPeer(args.clone())).unwrap())
        .send_and_await_response(10)
    else {
        return "Failed to get response from networking module".to_string();
    };
    let Ok(net::NetResponse::Peer(maybe_peer_id)) = rmp_serde::from_slice(&body) else {
        return "Got malformed response from networking module".to_string();
    };
    match maybe_peer_id {
        Some(peer_id) => format!(
            "peer identity for {}:\n    networking key: {}\n    routing: {:?}",
            peer_id.name, peer_id.networking_key, peer_id.routing
        ),
        None => format!("no PKI entry found with name {args}"),
    }
}
