use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

use uqbar_process_lib::{Address, ProcessId, Request, Response};
use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;

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
        },
        wit::Message::Request(wit::Request { ref ipc, .. }) => {
            match serde_json::from_slice(ipc)? {
                tt::TesterRequest::Run => {
                    wit::print_to_terminal(0, "test_runner: got Run");

                    let (_, response) = Request::new()
                        .target(make_vfs_address(&our)?)?
                        .ipc_bytes(serde_json::to_vec(&kt::VfsRequest {
                            drive: "tester:uqbar".into(),
                            action: kt::VfsAction::GetEntry("/".into()),
                        })?)
                        .send_and_await_response(5)??;

                    let wit::Message::Response((response, _)) = response else { panic!("") };
                    let kt::VfsResponse::GetEntry { children, .. } =
                        serde_json::from_slice(&response.ipc)? else { panic!("") };
                    let mut children: HashSet<_> = children.into_iter().collect();
                    children.remove("/manifest.json");
                    children.remove("/metadata.json");
                    children.remove("/tester.wasm");
                    children.remove("/test_runner.wasm");

                    wit::print_to_terminal(0, &format!("test_runner: running {:?}...", children));

                    for child in &children {
                        let child_process_id = match wit::spawn(
                            None,
                            child,
                            &wit::OnPanic::None, //  TODO: notify us
                            &wit::Capabilities::All,
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                wit::print_to_terminal(0, &format!("couldn't spawn {}: {}", child, e));
                                panic!("couldn't spawn"); //  TODO
                            }
                        };

                        let (_, response) = Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })?
                            .ipc_bytes(ipc.clone())
                            .send_and_await_response(5)??;

                        let wit::Message::Response((response, _)) = response else { panic!("") };
                        match serde_json::from_slice(&response.ipc)? {
                            tt::TesterResponse::Pass => {},
                            tt::TesterResponse::GetFullMessage(_) => {},
                            tt::TesterResponse::Fail { test, file, line, column } => {
                                fail!(test, file, line, column);
                            },
                        }
                    }

                    wit::print_to_terminal(0, &format!("test_runner: done running {:?}", children));

                    Response::new()
                        .ipc_bytes(serde_json::to_vec(&tt::TesterResponse::Pass).unwrap())
                        .send()
                        .unwrap();
                },
                tt::TesterRequest::KernelMessage(_) | tt::TesterRequest::GetFullMessage(_) => { unimplemented!() },
            }
            Ok(())
        },
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(0, "test_runner: begin");

        let our = Address::from_str(&our).unwrap();

        loop {
            match handle_message(&our) {
                Ok(()) => {},
                Err(e) => {
                    wit::print_to_terminal(0, format!(
                        "test_runner: error: {:?}",
                        e,
                    ).as_str());
                    fail!("test_runner");
                },
            };
        }
    }
}
