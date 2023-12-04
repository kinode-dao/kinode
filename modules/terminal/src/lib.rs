use anyhow::anyhow;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{println, Address, ProcessId, Request};

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct TerminalState {
    our: Address,
    current_target: Option<Address>,
}

fn serialize_message(message: &str) -> anyhow::Result<Vec<u8>> {
    Ok(message.as_bytes().to_vec())
}

fn parse_command(state: &mut TerminalState, line: &str) -> anyhow::Result<()> {
    let (head, tail) = line.split_once(" ").unwrap_or((&line, ""));
    match head {
        "" | " " => return Ok(()),
        // send a raw text message over the network to a node
        "!hi" => {
            let (node_id, message) = match tail.split_once(" ") {
                Some((s, t)) => (s, t),
                None => return Err(anyhow!("invalid command: \"{line}\"")),
            };
            let node_id = if node_id == "our" { state.our.name } else { node_id };
            Request::new()
                .target(Address::new(node_id, "net:sys:uqbar").unwrap())?
                .ipc(&message, serialize_message)?
                .expects_response(5)
                .send()?;
            Ok(())
        }
        // set the current target, so you can message it without specifying
        "!a" | "!app" => {
            let Ok(target) = Address::from_str(tail) else {
                return Err(anyhow!("invalid address: \"{tail}\""));
            };
            println!("current target set to {target}");
            state.current_target = Some(target);
        }
        // send a message to a specified app
        // if no current_target is set, require it,
        // otherwise use the current_target
        "!m" | "!message" => {
            if let Some(target) = state.current_target {
                Request::new()
                    .target(target)?
                    .ipc(&tail, serialize_message)?
                    .send()
            } else {
                let (target, ipc) = match tail.split_once(" ") {
                    Some((a, p)) => (a, p),
                    None => return Err(anyhow!("invalid command: \"{line}\"")),
                };
                let Ok(target) = Address::from_str(target) else {
                    return Err(anyhow!("invalid address: \"{target}\""));
                };
                Request::new()
                    .target(Address::new(node_id, process).unwrap())?
                    .ipc(&ipc, serialize_message)?
                    .send()
            }
        }
        _ => return Err(anyhow!("invalid command: \"{line}\"")),
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let state = TerminalState {
            our: Address::from_str(&our).unwrap(),
            current_target: None,
        };
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
                    if state.our.node != source.node || state.our.process != source.process {
                        continue;
                    }
                    match parse_command(&state, std::str::from_utf8(&ipc).unwrap_or_default()) {
                        Ok(()) => continue,
                        Err(e) => println!("terminal: {e}"),
                    }
                }
                wit::Message::Response((wit::Response { ipc, metadata, .. }, _)) => {
                    if let Ok(txt) = std::str::from_utf8(&ipc) {
                        println!("response from {source}: {txt}");
                    } else {
                        println!("response from {source}: {ipc:?}");
                    }
                }
            }
        }
    }
}
