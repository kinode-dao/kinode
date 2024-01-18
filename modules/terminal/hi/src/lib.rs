use kinode_process_lib::{await_message, call_init, println, Address, Message, Request, SendError};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(our: Address) {
    // TODO will need to package this up into a process lib function that makes it easy
    let Ok(Message::Request { body, .. }) = await_message() else {
        println!("got send error, failing out");
        return;
    };

    let tail = String::from_utf8(body).unwrap();

    let (node_id, message) = match tail.split_once(" ") {
        Some((s, t)) => (s, t),
        None => {
            println!("invalid command: \"{tail}\"");
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
        Err(SendError { kind, .. }) => {
            println!("hi: net error: {:?}", kind);
            return;
        }
    }
}
