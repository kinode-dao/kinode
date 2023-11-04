cargo_component_bindings::generate!();

use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

use bindings::component::uq_process::types::*;
use bindings::{create_capability, get_capability, Guest, has_capability, print_to_terminal, receive, send_request, send_response, share_capability, spawn};

mod kernel_types;
use kernel_types as kt;
mod process_lib;

struct Component;

#[derive(Debug, Serialize, Deserialize)]
enum TesterRequest {
    Run,
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
enum TesterError {
    #[error("RejectForeign")]
    RejectForeign,
    #[error("UnexpectedResponse")]
    UnexpectedResponse,
}

fn make_vfs_address(our: &Address) -> anyhow::Result<Address> {
    Ok(Address {
        node: our.node.clone(),
        process: ProcessId::from_str("vfs:sys:uqbar")?,
    })
}

fn handle_message (our: &Address) -> anyhow::Result<()> {
    let (source, message) = receive().unwrap();
    // let (source, message) = receive()?;

    if our.node != source.node {
        return Err(TesterError::RejectForeign.into());
    }

    match message {
        Message::Response(_) => {
            return Err(TesterError::UnexpectedResponse.into());
        },
        Message::Request(Request { ipc, .. }) => {
            match process_lib::parse_message_ipc(&ipc)? {
                TesterRequest::Run => {
                    print_to_terminal(0, "tester: got Run");
                    let (_, response) = process_lib::send_and_await_response(
                        &make_vfs_address(&our)?,
                        false,
                        serde_json::to_vec(&kt::VfsRequest {
                            drive: "tester:uqbar".into(),
                            action: kt::VfsAction::GetEntry("/".into()),
                        })?,
                        None,
                        None,
                        5,
                    )?;
                    let Message::Response((response, _)) = response else { panic!("") };
                    let kt::VfsResponse::GetEntry { children, .. } =
                        process_lib::parse_message_ipc(&response.ipc)? else { panic!("") };
                    let mut children: HashSet<_> = children.into_iter().collect();
                    children.remove("/manifest.json");
                    children.remove("/metadata.json");
                    children.remove("/tester.wasm");

                    print_to_terminal(0, &format!("tester: running {:?}...", children));

                    for child in &children {
                        let (_, response) = process_lib::send_and_await_response(
                            &make_vfs_address(&our)?,
                            false,
                            serde_json::to_vec(&kt::VfsRequest {
                                drive: "tester:uqbar".into(),
                                action: kt::VfsAction::GetEntryLength(child.into()),
                            })?,
                            None,
                            None,
                            5,
                        )?;
                        let Message::Response((response, _)) = response else { panic!("") };
                        let kt::VfsResponse::GetEntryLength(length) =
                            process_lib::parse_message_ipc(&response.ipc)? else { panic!("") };

                        print_to_terminal(0, &format!("tester: child {} length {:?}", child, length));

                        match spawn(
                            None,
                            child,
                            &OnPanic::None, //  TODO: notify us
                            &Capabilities::All,
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                print_to_terminal(0, &format!("couldn't spawn {}: {}", child, e));
                                panic!("couldn't spawn"); //  TODO
                            }
                        };
                    }
                    send_response(
                        &Response {
                            inherit: false,
                            ipc,
                            metadata: None,
                        },
                        None,
                    );
                },
            }
            Ok(())
        },
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "tester: begin");

        // orchestrate tests using external scripts
        //  -> must give drive cap to rpc
        // TODO: need read as well?
        let drive_cap = get_capability(
            &make_vfs_address(&our).unwrap(),
            &serde_json::to_string(&serde_json::json!({
                "kind": "write",
                "drive": "tester:uqbar",
            })).unwrap()
        ).unwrap();
        share_capability(&ProcessId::from_str("http_server:sys:uqbar").unwrap(), &drive_cap);

        loop {
            match handle_message(&our) {
                Ok(()) => {},
                Err(e) => {
                    print_to_terminal(0, format!(
                        "tester: error: {:?}",
                        e,
                    ).as_str());
                    // if let Some(e) = e.downcast_ref::<sq::SqliteError>() {
                    //     send_response(
                    //         &Response {
                    //             inherit: false,
                    //             ipc: serde_json::to_vec(&e).unwrap(),
                    //             metadata: None,
                    //         },
                    //         None,
                    //     );
                    // }
                },
            };
        }
    }
}
