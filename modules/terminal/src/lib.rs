use anyhow::anyhow;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{Address, ProcessId, Request, println};

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

fn serialize_message(message: &&str) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(message)?)
}

fn parse_command(our_name: &str, line: &str) -> anyhow::Result<()> {
    let (head, tail) = line.split_once(" ").unwrap_or((&line, ""));
    match head {
        "" | " " => return Ok(()),
        "!hi" => {
            let (node_id, message) = match tail.split_once(" ") {
                Some((s, t)) => (s, t),
                None => return Err(anyhow!("invalid command: \"{line}\"")),
            };
            let node_id = if node_id == "our" { our_name } else { node_id };
            Request::new()
                .target(Address::new(node_id, "net:sys:uqbar").unwrap())?
                .ipc(&message, serialize_message)?
                .expects_response(5)
                .send()?;
            Ok(())
        }
        "!message" => {
            let (node_id, tail) = match tail.split_once(" ") {
                Some((s, t)) => (s, t),
                None => return Err(anyhow!("invalid command: \"{line}\"")),
            };
            let (target_process, ipc) = match tail.split_once(" ") {
                Some((a, p)) => (a, p),
                None => return Err(anyhow!("invalid command: \"{line}\"")),
            };
            let node_id = if node_id == "our" { our_name } else { node_id };
            let process = ProcessId::from_str(target_process).unwrap_or_else(|_| {
                ProcessId::from_str(&format!("{}:sys:uqbar", target_process)).unwrap()
            });
            Request::new()
                .target(Address::new(node_id, process).unwrap())?
                .ipc(&ipc, serialize_message)?
                .send()?;
            Ok(())
        }
        _ => return Err(anyhow!("invalid command: \"{line}\"")),
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();
        println!("terminal: start");
        loop {
            let (source, message) = match wit::receive() {
                Ok((source, message)) => (source, message),
                Err((error, _context)) => {
                    println!("terminal: net error: {:?}!", error.kind);
                    continue;
                }
            };
            match message {
                wit::Message::Request(wit::Request {
                    expects_response,
                    ipc,
                    ..
                }) => {
                    if our.node != source.node || our.process != source.process {
                        continue;
                    }
                    match parse_command(&our.node, std::str::from_utf8(&ipc).unwrap_or_default()) {
                        Ok(()) => continue,
                        Err(e) => println!("terminal: {e}"),
                    }
                }
                wit::Message::Response((wit::Response { ipc, metadata, .. }, _)) => {
                    if let Ok(txt) = std::str::from_utf8(&ipc) {
                        println!("terminal: net response: {txt}");
                    }
                }
            }
        }
    }
}
