use kinode_process_lib::{call_init, net, println, Address, Message, Request};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

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
                "{}:\n    networking key: {}\n    routing: {:?}",
                peer_id.name, peer_id.networking_key, peer_id.routing
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    println!("identities of current connected peers:\n{identities}");
}
