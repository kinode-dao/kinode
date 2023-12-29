use indexmap::map::IndexMap;

use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::{
    await_message, call_init, get_capability, println, spawn, Address,
    Capabilities, Message, OnExit, ProcessId, Request, Response,
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
        process: ProcessId::from_str("vfs:sys:uqbar")?,
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
        Message::Response { source, ipc, .. } => {
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
        Message::Request { source, ipc, .. } => {
            match serde_json::from_slice(&ipc)? {
                tt::TesterRequest::Run {
                    input_node_names,
                    test_timeout,
                } => {
                    println!("tester: got Run");

                    assert!(input_node_names.len() >= 1);
                    *node_names = input_node_names.clone();

                    if our.node != node_names[0] {
                        Response::new()
                            .ipc(serde_json::to_vec(&tt::TesterResponse::Pass).unwrap())
                            .send()
                            .unwrap();
                    } else {
                        // we are master node
                        let child = "/tester:uqbar/pkg/test_runner.wasm";
                        let child_process_id = match spawn(
                            None,
                            child,
                            OnExit::None, //  TODO: notify us
                            &Capabilities::All,
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

call_init!(init);
fn init(our: Address) {
    println!("tester: begin");

    let mut messages: Messages = IndexMap::new();
    let mut node_names: Vec<String> = Vec::new();

    // orchestrate tests using external scripts
    //  -> must give drive cap to rpc
    let _ = Request::new()
        .target(make_vfs_address(&our).unwrap())
        .ipc(serde_json::to_vec(&kt::VfsRequest {
            path: "/tester:uqbar/tests".into(),
            action: kt::VfsAction::CreateDrive,
        }).unwrap())
        .send_and_await_response(5)
        .unwrap()
        .unwrap();
    let drive_cap = get_capability(
        &make_vfs_address(&our).unwrap(),
        &serde_json::to_string(&serde_json::json!({
            "kind": "write",
            "drive": "/tester:uqbar/tests",
        }))
        .expect("couldn't serialize"),
    )
    .expect("couldn't get drive cap");
    // TODO sharing capabilities not allowed in the new paradigm
    // share_capability(
    //     &ProcessId::from_str("http_server:sys:uqbar").expect("couldn't make pid"),
    //     &drive_cap,
    // );

    loop {
        match handle_message(&our, &mut messages, &mut node_names) {
            Ok(()) => {}
            Err(e) => {
                println!("tester: error: {:?}", e,);
            }
        };
    }
}
