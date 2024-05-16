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
    let Ok(namehash) = String::from_utf8(args) else {
        println!("argument must be a string");
        return;
    };
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetName(namehash.clone())).unwrap())
        .send_and_await_response(5)
    else {
        println!("failed to get name from networking module");
        return;
    };
    let Ok(net::NetResponse::Name(maybe_name)) = rmp_serde::from_slice(&body) else {
        println!("got malformed response from networking module");
        return;
    };
    match maybe_name {
        Some(name) => println!("{namehash}: {name}"),
        None => println!("no name found for {namehash}"),
    }
}
