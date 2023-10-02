cargo_component_bindings::generate!();
mod process_lib;
struct Component;
use bindings::{component::uq_process::types::*, Guest, print_to_terminal, receive, send_request};

fn parse_command(our_name: &str, line: String) {
    let (head, tail) = line.split_once(" ").unwrap_or((&line, ""));
    match head {
        "" | " " => {}
        "!hi" => {
            let (target, message) = match tail.split_once(" ") {
                Some((s, t)) => (s, t),
                None => {
                    print_to_terminal(0, &format!("invalid command: \"{}\"", line));
                    return;
                }
            };
            send_request(
                &Address {
                    node: if target == "our" {
                        our_name.into()
                    } else {
                        target.into()
                    },
                    process: ProcessId::Name("net".into()),
                },
                &Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(message.into()),
                    metadata: None,
                },
                None,
                None,
            );
        }
        "!message" => {
            let (target_node, tail) = match tail.split_once(" ") {
                Some((s, t)) => (s, t),
                None => {
                    print_to_terminal(0, &format!("invalid command: \"{}\"", line));
                    return;
                }
            };
            let (target_process, ipc) = match tail.split_once(" ") {
                Some((a, p)) => (a, p),
                None => {
                    print_to_terminal(0, &format!("invalid command: \"{}\"", line));
                    return;
                }
            };
            //  TODO: why does this work but using the API below does not?
            //        Is it related to passing json in rather than a Serialize type?
            send_request(
                &Address {
                    node: if target_node == "our" {
                        our_name.into()
                    } else {
                        target_node.into()
                    },
                    process: ProcessId::Name(target_process.into()),
                },
                &Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(ipc.into()),
                    metadata: None,
                },
                None,
                None,
            );
        }
        _ => {
            print_to_terminal(0, &format!("invalid command: \"{line}\""));
        }
    }
}

impl Guest for Component {
    fn init(our: Address) {
        assert_eq!(our.process, ProcessId::Name("terminal".into()));
        print_to_terminal(0, &format!("terminal: running"));
        loop {
            let message = match receive() {
                Ok((source, message)) => {
                    if our.node != source.node {
                        continue;
                    }
                    message
                }
                Err((error, _context)) => {
                    print_to_terminal(0, &format!("net error: {:?}!", error.kind));
                    continue;
                }
            };
            match message {
                Message::Request(Request {
                    expects_response,
                    ipc,
                    ..
                }) => {
                    let Some(command) = ipc else {
                        continue;
                    };
                    parse_command(&our.node, command);
                }
                _ => continue
            }
        }
    }
}
