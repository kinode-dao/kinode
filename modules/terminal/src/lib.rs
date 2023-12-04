use anyhow::anyhow;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{println, Address, Request};

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
        "/hi" => {
            let (node_id, message) = match tail.split_once(" ") {
                Some((s, t)) => (s, t),
                None => return Err(anyhow!("invalid command: \"{line}\"")),
            };
            let node_id = if node_id == "our" { &state.our.node } else { node_id };
            Request::new()
                .target((node_id, "net", "sys", "uqbar"))
                .ipc(message)
                .expects_response(5)
                .send()?;
            Ok(())
        }
        // set the current target, so you can message it without specifying
        "/a" | "/app" => {
            if tail == "" || tail == "clear" {
                state.current_target = None;
                println!("current target cleared");
                return Ok(());
            }
            let Ok(target) = Address::from_str(tail) else {
                return Err(anyhow!("invalid address: \"{tail}\""));
            };
            println!("current target set to {target}");
            state.current_target = Some(target);
            Ok(())
        }
        // send a message to a specified app
        // if no current_target is set, require it,
        // otherwise use the current_target
        "/m" | "/message" => {
            if let Some(target) = &state.current_target {
                Request::new()
                    .target(target.clone())
                    .ipc(tail)
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
                    .target(target)
                    .ipc(ipc)
                    .send()
            }
        }
        _ => return Err(anyhow!("invalid command: \"{line}\"")),
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let mut state = TerminalState {
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
                    ipc,
                    ..
                }) => {
                    if state.our.node != source.node || state.our.process != source.process {
                        continue;
                    }
                    match parse_command(&mut state, std::str::from_utf8(&ipc).unwrap_or_default()) {
                        Ok(()) => continue,
                        Err(e) => println!("terminal: {e}"),
                    }
                }
                wit::Message::Response((wit::Response { ipc, .. }, _)) => {
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
