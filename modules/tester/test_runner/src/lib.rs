use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{spawn, Address, Message, OnExit, ProcessId, Request, Response};

mod tester_types;
use tester_types as tt;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

fn make_vfs_address(our: &wit::Address) -> anyhow::Result<Address> {
    Ok(wit::Address {
        node: our.node.clone(),
        process: ProcessId::from_str("vfs:sys:uqbar")?,
    })
}

fn handle_message(our: &Address) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(tt::TesterError::RejectForeign.into());
    }

    match message {
        wit::Message::Response(_) => {
            return Err(tt::TesterError::UnexpectedResponse.into());
        }
        wit::Message::Request(wit::Request { ref ipc, .. }) => {
            match serde_json::from_slice(ipc)? {
                tt::TesterRequest::Run { test_timeout, .. } => {
                    wit::print_to_terminal(0, "test_runner: got Run");

                    let response = Request::new()
                        .target(make_vfs_address(&our)?)
                        .ipc(serde_json::to_vec(&kt::VfsRequest {
                            path: "/tester:uqbar/tests".into(),
                            action: kt::VfsAction::ReadDir,
                        })?)
                        .send_and_await_response(test_timeout)?
                        .unwrap();

                    let Message::Response { ipc: vfs_ipc, .. } = response else {
                        panic!("")
                    };
                    let kt::VfsResponse::ReadDir(children) =
                        serde_json::from_slice(&vfs_ipc)?
                    else {
                        wit::print_to_terminal(
                            0,
                            &format!(
                                "{:?}",
                                serde_json::from_slice::<serde_json::Value>(&vfs_ipc)?,
                            ),
                        );
                        panic!("")
                    };

                    wit::print_to_terminal(0, &format!("test_runner: running {:?}...", children));

                    for child in &children {
                        let child_process_id = match spawn(
                            None,
                            &child.path, // TODO not sure if this is right
                            OnExit::None, //  TODO: notify us
                            &wit::Capabilities::All,
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                wit::print_to_terminal(
                                    0,
                                    &format!("couldn't spawn {}: {}", child.path, e),
                                );
                                panic!("couldn't spawn"); //  TODO
                            }
                        };

                        let response = Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })
                            .ipc(ipc.clone())
                            .send_and_await_response(test_timeout)?
                            .unwrap();

                        let Message::Response { ipc, .. } = response else {
                            panic!("")
                        };
                        match serde_json::from_slice(&ipc)? {
                            tt::TesterResponse::Pass => {}
                            tt::TesterResponse::GetFullMessage(_) => {}
                            tt::TesterResponse::Fail {
                                test,
                                file,
                                line,
                                column,
                            } => {
                                fail!(test, file, line, column);
                            }
                        }
                    }

                    wit::print_to_terminal(0, &format!("test_runner: done running {:?}", children));

                    Response::new()
                        .ipc(serde_json::to_vec(&tt::TesterResponse::Pass).unwrap())
                        .send()
                        .unwrap();
                }
                tt::TesterRequest::KernelMessage(_) | tt::TesterRequest::GetFullMessage(_) => {
                    unimplemented!()
                }
            }
            Ok(())
        }
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(0, "test_runner: begin");

        let our = Address::from_str(&our).unwrap();

        // TODO fix
        // wit::create_capability(
        //     &ProcessId::new(Some("vfs"), "sys", "uqbar"),
        //     &"\"messaging\"".into(),
        // );

        loop {
            match handle_message(&our) {
                Ok(()) => {}
                Err(e) => {
                    wit::print_to_terminal(0, format!("test_runner: error: {:?}", e,).as_str());
                    fail!("test_runner");
                }
            };
        }
    }
}
