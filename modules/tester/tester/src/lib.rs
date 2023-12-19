use indexmap::map::IndexMap;

use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{spawn, Address, OnExit, ProcessId, Request, Response};

mod tester_types;
use tester_types as tt;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

type Messages = IndexMap<kt::Message, tt::KernelMessage>;

fn make_vfs_address(our: &wit::Address) -> anyhow::Result<Address> {
    Ok(wit::Address {
        node: our.node.clone(),
        process: ProcessId::from_str("vfs:sys:uqbar")?,
    })
}

fn handle_message(
    our: &Address,
    _messages: &mut Messages,
    node_names: &mut Vec<String>,
) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    match message {
        wit::Message::Response((wit::Response { ipc, .. }, _)) => {
            match serde_json::from_slice(&ipc)? {
                tt::TesterResponse::Pass | tt::TesterResponse::Fail { .. } => {
                    if (source.process.package_name != "tester")
                        | (source.process.publisher_node != "uqbar")
                    {
                        return Err(tt::TesterError::UnexpectedResponse.into());
                    }
                    Response::new().ipc(ipc).send().unwrap();
                }
                tt::TesterResponse::GetFullMessage(_) => {
                    unimplemented!()
                }
            }
            Ok(())
        }
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
                tt::TesterRequest::Run {
                    input_node_names,
                    test_timeout,
                } => {
                    wit::print_to_terminal(0, "tester: got Run");

                    assert!(input_node_names.len() >= 1);
                    *node_names = input_node_names.clone();

                    if our.node != node_names[0] {
                        Response::new()
                            .ipc(serde_json::to_vec(&tt::TesterResponse::Pass).unwrap())
                            .send()
                            .unwrap();
                    } else {
                        // we are master node
                        let child = "/test_runner.wasm";
                        let child_process_id = match spawn(
                            None,
                            child,
                            OnExit::None, //  TODO: notify us
                            &wit::Capabilities::All,
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                wit::print_to_terminal(
                                    0,
                                    &format!("couldn't spawn {}: {}", child, e),
                                );
                                panic!("couldn't spawn"); //  TODO
                            }
                        };
                        Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })
                            .ipc(ipc)
                            .expects_response(test_timeout)
                            .send()?;
                    }
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
        wit::print_to_terminal(0, "tester: begin");

        let our = Address::from_str(&our).unwrap();
        let mut messages: Messages = IndexMap::new();
        let mut node_names: Vec<String> = Vec::new();

        // orchestrate tests using external scripts
        //  -> must give drive cap to rpc
        let drive_cap = wit::get_capability(
            &make_vfs_address(&our).unwrap(),
            &serde_json::to_string(&serde_json::json!({
                "kind": "write",
                "drive": "tester:uqbar",
            }))
            .unwrap(),
        )
        .unwrap();
        wit::share_capability(
            &ProcessId::from_str("http_server:sys:uqbar").unwrap(),
            &drive_cap,
        );

        loop {
            match handle_message(&our, &mut messages, &mut node_names) {
                Ok(()) => {}
                Err(e) => {
                    wit::print_to_terminal(0, format!("tester: error: {:?}", e,).as_str());
                    fail!("tester");
                }
            };
        }
    }
}
