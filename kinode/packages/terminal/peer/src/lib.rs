use kinode_process_lib::{
    await_next_message_body, call_init, net, println, Address, Message, Request,
};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process",
});

call_init!(init);
fn init(_our: Address) {
    let Ok(args) = await_next_message_body() else {
        println!("failed to get args");
        return;
    };
    let Ok(name) = String::from_utf8(args) else {
        println!("argument must be a string");
        return;
    };
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetPeer(name.clone())).unwrap())
        .send_and_await_response(5)
    else {
        println!("failed to get response from networking module");
        return;
    };
    let Ok(net::NetResponse::Peer(maybe_peer_id)) = rmp_serde::from_slice(&body) else {
        println!("got malformed response from networking module");
        return;
    };
    match maybe_peer_id {
        Some(peer_id) => println!(
            "peer identity for {}:\n    networking key: {}\n    routing: {:?}\n    routers: {:?}",
            peer_id.name, peer_id.networking_key, peer_id.ws_routing, peer_id.allowed_routers
        ),
        None => println!("no PKI entry found with name {name}"),
    }
}
