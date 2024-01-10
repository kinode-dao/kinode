use indexmap::map::IndexMap;

use nectar_process_lib::kernel_types as kt;
use nectar_process_lib::{
    await_message, call_init, our_capabilities, println, spawn, vfs, Address, Message,
    OnExit, ProcessId, Request, Response,
};

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

fn make_vfs_address(our: &Address) -> anyhow::Result<Address> {
    Ok(Address {
        node: our.node.clone(),
        process: "vfs:sys:nectar".parse()?,
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
                        | (source.process.publisher_node != "nectar")
                    {
                        return Err(tt::TesterError::UnexpectedResponse.into());
                    }
                    Response::new().body(body).send().unwrap();
                }
                tt::TesterResponse::GetFullMessage(_) => {
                    unimplemented!()
                }
            }
            Ok(())
        }
        Message::Request { body, .. } => {
            match serde_json::from_slice(&body)? {
                tt::TesterRequest::Run {
                    input_node_names,
                    test_timeout,
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
                        let child = "/tester:nectar/pkg/test_runner.wasm";
                        let child_process_id = match spawn(
                            None,
                            child,
                            OnExit::None, //  TODO: notify us
                            our_capabilities(),
                            vec!["vfs:sys:nectar".parse::<ProcessId>().unwrap()],
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                println!("couldn't spawn {}: {}", child, e);
                                panic!("couldn't spawn"); //  TODO
                            }
                        };
                        Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })
                            .body(body)
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

call_init!(init);
fn init(our: Address) {
    println!("tester: begin");

    let mut messages: Messages = IndexMap::new();
    let mut node_names: Vec<String> = Vec::new();
    let _ = Request::new()
        .target(make_vfs_address(&our).unwrap())
        .body(
            serde_json::to_vec(&vfs::VfsRequest {
                path: "/tester:nectar/tests".into(),
                action: vfs::VfsAction::CreateDrive,
            })
            .unwrap(),
        )
        .send_and_await_response(5)
        .unwrap()
        .unwrap();

    // orchestrate tests using external scripts
    //  -> must give drive cap to rpc
    let _ = Request::new()
        .target(("our", "kernel", "sys", "nectar"))
        .body(
            serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                target: ProcessId::new(Some("http_server"), "sys", "nectar"),
                capabilities: vec![kt::Capability {
                    issuer: Address::new(
                        our.node.clone(),
                        ProcessId::new(Some("vfs"), "sys", "nectar"),
                    ),
                    params: serde_json::json!({
                        "kind": "write",
                        "drive": "/tester:nectar/tests",
                    })
                    .to_string(),
                }],
            })
            .unwrap(),
        )
        .send()
        .unwrap();

    loop {
        match handle_message(&our, &mut messages, &mut node_names) {
            Ok(()) => {}
            Err(e) => {
                println!("tester: error: {:?}", e,);
            }
        };
    }
}
