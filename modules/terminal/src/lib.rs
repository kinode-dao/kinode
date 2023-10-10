cargo_component_bindings::generate!();
use bindings::{component::uq_process::types::*, print_to_terminal, receive, send_request, Guest};

#[allow(dead_code)]
mod process_lib;

struct Component;

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
                    process: ProcessId::from_str("net:sys:uqbar").unwrap(),
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
            //
            print_to_terminal(0, &format!("terminal: {}\r", target_process));
            print_to_terminal(0, &format!("terminal: {:?}\r", ProcessId::from_str(target_process).unwrap_or(ProcessId::from_str(&format!("{}:sys:uqbar", target_process)).unwrap())));
            send_request(
                &Address {
                    node: if target_node == "our" {
                        our_name.into()
                    } else {
                        target_node.into()
                    },
                    process: ProcessId::from_str(target_process).unwrap_or(
                        ProcessId::from_str(&format!("{}:sys:uqbar", target_process)).unwrap(),
                    ),
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
        assert_eq!(our.process.to_string(), "terminal:terminal:uqbar");
        print_to_terminal(1, &format!("terminal: start"));
        loop {
            let (source, message) = match receive() {
                Ok((source, message)) => (source, message),
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
                    if our.node != source.node || our.process != source.process {
                        continue;
                    }
                    let Some(command) = ipc else {
                        continue;
                    };
                    parse_command(&our.node, command);
                }
                _ => continue,
            }
        }
    }
}
