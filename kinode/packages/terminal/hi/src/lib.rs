use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, Request, SendError, SendErrorKind,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(our: Address) {
    let Ok(args) = await_next_request_body() else {
        println!("failed to get args");
        return;
    };

    let tail = String::from_utf8(args).unwrap();
    if tail.is_empty() {
        println!("Send a Message to another node's terminal");
        println!("\x1b[1mUsage:\x1b[0m hi <node_id> <message>");
        return;
    }

    let (node_id, message) = match tail.split_once(" ") {
        Some((s, t)) => (s, t),
        None => {
            println!("invalid command, please provide a message");
            return;
        }
    };
    let node_id = if node_id == "our" { &our.node } else { node_id };
    match Request::new()
        .target((node_id, "net", "distro", "sys"))
        .body(message)
        .send_and_await_response(5)
        .unwrap()
    {
        Ok(msg) => {
            if let Ok(txt) = std::str::from_utf8(&msg.body()) {
                println!("response from {node_id}: {txt}");
            } else {
                println!("response from {node_id}: {:?}", msg.body());
            }
        }
        Err(SendError { kind, .. }) => match kind {
            SendErrorKind::Timeout => {
                println!("message to {node_id} timed out");
            }
            SendErrorKind::Offline => {
                println!("{node_id} is offline or does not exist");
            }
        },
    }
}
