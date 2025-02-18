use crate::hyperware::process::tester::{
    FailResponse, Request as TesterRequest, Response as TesterResponse, RunRequest,
};
use hyperware_process_lib::kernel_types as kt;
use hyperware_process_lib::{
    await_message, call_init, our_capabilities, println, spawn, vfs, Address, Capability, Message,
    OnExit, ProcessId, Request, Response,
};
use std::collections::HashMap;

mod tester_lib;

wit_bindgen::generate!({
    path: "target/wit",
    world: "tester-sys-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

const SETUP_PATH: &str = "/tester:sys/setup";
const TESTS_PATH: &str = "/tester:sys/tests";

fn handle_response(message: &Message) -> anyhow::Result<()> {
    let TesterResponse::Run(_) = message.body().try_into()?;
    let source = message.source();
    if (source.process.package_name != "tester") || (source.process.publisher_node != "sys") {
        println!(
            "got Response from unexpected source: {}; must be in package test:sys",
            source,
        );
        fail!("tester");
    }
    Response::new().body(message.body()).send().unwrap();
    Ok(())
}

fn read_caps_by_child(
    dir_prefix: &str,
    files: &mut Vec<vfs::DirEntry>,
) -> anyhow::Result<HashMap<String, HashMap<String, Vec<String>>>> {
    // find DirEntry with path caps_file_path
    let caps_file_path = format!("{}/capabilities.json", dir_prefix);
    let caps_index = files.iter().position(|i| *i.path == *caps_file_path);

    let caps_by_child: HashMap<String, HashMap<String, Vec<String>>> = match caps_index {
        None => HashMap::new(),
        Some(caps_index) => {
            files.remove(caps_index);
            let file = vfs::file::open_file(&caps_file_path, false, None)?;
            let file_contents = file.read()?;
            serde_json::from_slice(&file_contents)?
        }
    };
    Ok(caps_by_child)
}

fn handle_request(
    our: &Address,
    message: &Message,
    node_names: &mut Vec<String>,
) -> anyhow::Result<()> {
    let TesterRequest::Run(RunRequest {
        input_node_names,
        ref test_names,
        test_timeout,
    }) = message.body().try_into()?;
    println!("got Run");

    assert!(input_node_names.len() >= 1);
    *node_names = input_node_names.clone();

    if our.node != node_names[0] {
        // we are not the master node
        Response::new()
            .body(TesterResponse::Run(Ok(())))
            .send()
            .unwrap();
        return Ok(());
    }

    // we are the master node
    let dir_prefix = "tester:sys/tests";

    let response = Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: dir_prefix.into(),
            action: vfs::VfsAction::ReadDir,
        })?)
        .send_and_await_response(test_timeout)??;

    let Message::Response { body: vfs_body, .. } = response else {
        fail!("tester");
    };
    let vfs::VfsResponse::ReadDir(mut children) = serde_json::from_slice(&vfs_body)? else {
        println!(
            "{:?}",
            serde_json::from_slice::<serde_json::Value>(&vfs_body)?
        );
        fail!("tester");
    };

    for test_name in test_names {
        let test_entry = vfs::DirEntry {
            path: format!("{}/{}.wasm", dir_prefix, test_name),
            file_type: vfs::FileType::File,
        };
        if !children.contains(&test_entry) {
            return Err(anyhow::anyhow!(
                "test {} not found amongst {:?}",
                test_name,
                children,
            ));
        }
    }

    let caps_by_child = read_caps_by_child(dir_prefix, &mut children)?;

    println!("tester: running {:?}...", children);

    for test_name in test_names {
        let test_path = format!("{}/{}.wasm", dir_prefix, test_name);
        let (mut request_caps, grant_caps) = caps_by_child
            .get(test_name)
            .and_then(|caps_map| {
                Some((
                    caps_map["request_capabilities"]
                        .iter()
                        .map(|cap| {
                            serde_json::from_str(cap).unwrap_or_else(|_| {
                                Capability::new(
                                    Address::new(our.node(), cap.parse::<ProcessId>().unwrap()),
                                    "\"messaging\"",
                                )
                            })
                        })
                        .collect(),
                    caps_map["grant_capabilities"]
                        .iter()
                        .map(|cap| {
                            serde_json::from_str::<(ProcessId, String)>(cap).unwrap_or_else(|_| {
                                (
                                    cap.parse::<ProcessId>().unwrap(),
                                    "\"messaging\"".to_string(),
                                )
                            })
                        })
                        .collect(),
                ))
            })
            .unwrap_or((vec![], vec![]));
        println!("tester: request_caps: {request_caps:?}\ntester: grant_caps: {grant_caps:?}");
        request_caps.extend(our_capabilities());
        let child_process_id = match spawn(
            None,
            &test_path,
            OnExit::None, //  TODO: notify us
            request_caps,
            grant_caps,
            false, // not public
        ) {
            Ok(child_process_id) => child_process_id,
            Err(e) => {
                println!("couldn't spawn {}: {}", test_path, e);
                fail!("tester");
            }
        };

        let response = Request::new()
            .target(Address {
                node: our.node.clone(),
                process: child_process_id,
            })
            .body(message.body())
            .send_and_await_response(test_timeout)??;

        if response.is_request() {
            fail!("tester");
        };
        let TesterResponse::Run(result) = response.body().try_into()?;
        if let Err(FailResponse {
            test,
            file,
            line,
            column,
        }) = result
        {
            fail!(test, file, line, column);
        }
    }

    println!("test_runner: done running {:?}", children);

    Response::new().body(TesterResponse::Run(Ok(()))).send()?;

    Ok(())
}

fn handle_message(our: &Address, node_names: &mut Vec<String>) -> anyhow::Result<()> {
    let Ok(message) = await_message() else {
        return Ok(());
    };

    if !message.is_request() {
        return handle_response(&message);
    }
    return handle_request(our, &message, node_names);
}

call_init!(init);
fn init(our: Address) {
    let mut node_names: Vec<String> = Vec::new();
    for path in [SETUP_PATH, TESTS_PATH] {
        match Request::new()
            .target(("our", "vfs", "distro", "sys"))
            .body(
                serde_json::to_vec(&vfs::VfsRequest {
                    path: path.into(),
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
                    target: ProcessId::new(Some("http-server"), "distro", "sys"),
                    capabilities: vec![kt::Capability {
                        issuer: Address::new(
                            our.node.clone(),
                            ProcessId::new(Some("vfs"), "distro", "sys"),
                        ),
                        params: serde_json::json!({
                            "kind": "write",
                            "drive": path,
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
    }

    loop {
        match handle_message(&our, &mut node_names) {
            Ok(()) => {}
            Err(e) => {
                println!("tester: error: {e:?}");
                fail!("tester");
            }
        };
    }
}
