use hyperware_process_lib::{script, Address, Request, SendError, SendErrorKind};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

const USAGE: &str = "\x1b[1mUsage:\x1b[0m hi <node_id> <message>";

script!(init);
fn init(our: Address, args: String) -> String {
    if args.is_empty() {
        return format!("Send a text message to another node's terminal.\n{USAGE}");
    }

    let (node_id, message) = match args.split_once(" ") {
        Some((s, t)) => (s, t),
        None => return format!("Not enough arguments given.\n{USAGE}"),
    };
    let node_id = if node_id == "our" { &our.node } else { node_id };
    match Request::to((node_id, "net", "distro", "sys"))
        .body(message)
        .send_and_await_response(10)
        .unwrap()
    {
        Ok(msg) => {
            if let Ok(txt) = std::str::from_utf8(&msg.body()) {
                format!("response from {node_id}: {txt}")
            } else {
                format!("malformed response from {node_id}")
            }
        }
        Err(SendError { kind, .. }) => match kind {
            SendErrorKind::Timeout => {
                format!("message to {node_id} timed out")
            }
            SendErrorKind::Offline => {
                format!("{node_id} is offline or does not exist")
            }
        },
    }
}
