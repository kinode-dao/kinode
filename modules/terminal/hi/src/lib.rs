use kinode_process_lib::{
    await_next_request_body, call_init, println, Address, Request, SendError,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

call_init!(init);

fn init(our: Address) {
    let Ok(args) = await_next_request_body() else {
        println!("hi: failed to get args, aborting");
        return;
    };

    let tail = String::from_utf8(args).unwrap();

    let (node_id, message) = match tail.split_once(" ") {
        Some((s, t)) => (s, t),
        None => {
            println!("hi: invalid command, please provide a message");
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
