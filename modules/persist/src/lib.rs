cargo_component_bindings::generate!();

use bindings::component::uq_process::types::*;
use bindings::{Address, Guest, print_to_terminal, receive, send_response};
use serde::{Deserialize, Serialize};

mod process_lib;

struct Component;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    val: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FsAction {
    Write,
    Replace(u128),
    Append(Option<u128>),
    Read(u128),
    ReadChunk(ReadChunkRequest),
    Delete(u128),
    Length(u128),
    //  process state management
    GetState,
    SetState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReadChunkRequest {
    pub file_uuid: u128,
    pub start: u64,
    pub length: u64,
}

#[derive(Debug, Serialize, Deserialize)]
enum PersistRequest {
    Get,
    Set { new: u64 },
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FsResponse {
    //  bytes are in payload_bytes
    Read(u128),
    ReadChunk(u128),
    Write(u128),
    Append(u128),
    Delete(u128),
    Length(u64),
    GetState,
    SetState
    //  use FileSystemError
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(1, "persist: start");

        let mut state = State {
            val: None
        };
        match process_lib::get_state(our.node.clone()) {
            None => {
                print_to_terminal(
                    0,
                    "persist: no previous boot state",
                );
            },
            Some(p) => {
                match bincode::deserialize(&p.bytes) {
                    Err(e) => print_to_terminal(
                        0,
                        &format!("persist: failed to deserialize payload from fs: {}", e),
                    ),
                    Ok(s) => {
                        state = s;
                    },
                }
            },
        }

        process_lib::await_set_state(our.node.clone(), &state);

        loop {
            let Ok((_source, message)) = receive() else {
                print_to_terminal(0, "persist: got network error");
                continue;
            };

            match message {
                Message::Request(request) => {
                    let persist_msg = serde_json::from_str::<PersistRequest>(&request.clone().ipc.unwrap_or_default());
                    let Ok(msg) = persist_msg else {
                        print_to_terminal(0, &format!("persist: got invalid request {:?}", request.clone()));
                        continue;
                    };
                    match msg {
                        PersistRequest::Get => {
                            print_to_terminal(0, &format!("persist: Get state: {:?}", state));
                        },
                        PersistRequest::Set { new } => {
                            print_to_terminal(1, "persist: got Set request");
                            state.val = Some(new);
                            process_lib::await_set_state(our.node.clone(), &state);
                            // let _ = process_lib::set_state(our.node.clone(), bincode::serialize(&state).unwrap());
                            print_to_terminal(1, "persist: done Set request");
                        },
                    }
                    send_response(
                        &Response {
                            ipc: None,
                            metadata: None,
                        },
                        None,
                    );
                },
                _ => {
                    print_to_terminal(0, "persist: got unexpected message");
                    continue;
                }
            }

        }
    }
}
