use indexmap::map::IndexMap;

use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::{
    await_message, call_init, our_capabilities, println, spawn, vfs, Address, Message, OnExit,
    ProcessId, Request, Response,
};

mod tester_types;
use tester_types as tt;

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

type Messages = IndexMap<kt::Message, tt::KernelMessage>;

fn make_vfs_address(our: &Address) -> anyhow::Result<Address> {
    Ok(Address {
        node: our.node.clone(),
        process: "vfs:distro:sys".parse()?,
    })
}

fn handle_message(
    our: &Address,
    _messages: &mut Messages,
    node_names: &mut Vec<String>,
) -> anyhow::Result<()> {
    let Ok(message) = await_message() else {
        return Ok(());
    };

    match message {
        Message::Response { source, body, .. } => {
            match serde_json::from_slice(&body)? {
                tt::TesterResponse::Pass | tt::TesterResponse::Fail { .. } => {
                    if (source.process.package_name != "tester")
                        | (source.process.publisher_node != "sys")
                    {
                        return Err(tt::TesterError::UnexpectedResponse.into());
                    }
                    Response::new().body(body).send().unwrap();
                }
                tt::TesterResponse::GetFullMessage(_) => {
                    fail!("tester");
                }
            }
            Ok(())
        }
        Message::Request { body, .. } => {
            match serde_json::from_slice(&body)? {
                tt::TesterRequest::Run {
                    input_node_names,
                    test_timeout,
                    ..
                } => {
                    println!("tester: got Run");

                    assert!(input_node_names.len() >= 1);
                    *node_names = input_node_names.clone();

                    if our.node != node_names[0] {
                        Response::new()
                            .body(serde_json::to_vec(&tt::TesterResponse::Pass).unwrap())
                            .send()
                            .unwrap();
                    } else {
                        // we are master node
                        let child = "/tester:sys/pkg/test_runner.wasm";
                        let child_process_id = match spawn(
                            None,
                            child,
                            OnExit::None, //  TODO: notify us
                            our_capabilities(),
                            vec!["vfs:distro:sys".parse::<ProcessId>().unwrap()],
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                println!("couldn't spawn {}: {}", child, e);
                                fail!("tester");
                            }
                        };
                        Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })
                            .body(body)
                            .expects_response(test_timeout * 2)
                            .send()?;
                    }
                }
                tt::TesterRequest::KernelMessage(_) | tt::TesterRequest::GetFullMessage(_) => {
                    fail!("tester");
                }
            }
            Ok(())
        }
    }
}

call_init!(init);
fn init(our: Address) {
    println!("{}: begin", our);

    let mut messages: Messages = IndexMap::new();
    let mut node_names: Vec<String> = Vec::new();
    match Request::new()
        .target(make_vfs_address(&our).unwrap())
        .body(
            serde_json::to_vec(&vfs::VfsRequest {
                path: "/tester:sys/tests".into(),
                action: vfs::VfsAction::CreateDrive,
            })
            .unwrap(),
        )
        .send_and_await_response(5)
    {
        Err(_) => {
            fail!("tester");
        }
        Ok(r) => {
            if r.is_err() {
                fail!("tester");
            }
        }
    }

    // orchestrate tests using external scripts
    //  -> must give drive cap to rpc
    let sent = Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(
            serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                target: ProcessId::new(Some("http_server"), "distro", "sys"),
                capabilities: vec![kt::Capability {
                    issuer: Address::new(
                        our.node.clone(),
                        ProcessId::new(Some("vfs"), "distro", "sys"),
                    ),
                    params: serde_json::json!({
                        "kind": "write",
                        "drive": "/tester:sys/tests",
                    })
                    .to_string(),
                }],
            })
            .unwrap(),
        )
        .send();
    if sent.is_err() {
        fail!("tester");
    }

    loop {
        match handle_message(&our, &mut messages, &mut node_names) {
            Ok(()) => {}
            Err(e) => {
                println!("tester: error: {:?}", e,);
                fail!("tester");
            }
        };
    }
}
