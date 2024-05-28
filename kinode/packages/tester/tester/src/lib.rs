use std::collections::HashMap;
use std::str::FromStr;

use crate::kinode::process::tester::{
    Request as TesterRequest, Response as TesterResponse, RunRequest, FailResponse,
};
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::{
    await_message, call_init, our_capabilities, println, spawn, vfs, Address, Message, OnExit,
    ProcessId, Request, Response,
};

mod tester_lib;

wit_bindgen::generate!({
    path: "target/wit",
    world: "tester-sys-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

fn make_vfs_address(our: &Address) -> anyhow::Result<Address> {
    Ok(Address {
        node: our.node.clone(),
        process: "vfs:distro:sys".parse()?,
    })
}

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
    children: &mut Vec<vfs::DirEntry>,
) -> anyhow::Result<HashMap<String, Vec<String>>> {
    let caps_file_path = format!("{}/grant_capabilities.json", dir_prefix);
    let caps_index = children.iter().position(|i| *i.path == *caps_file_path);
    let caps_by_child: HashMap<String, Vec<String>> =
        match caps_index {
            None => HashMap::new(),
            Some(caps_index) => {
                children.remove(caps_index);
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
        .target(make_vfs_address(&our)?)
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: dir_prefix.into(),
            action: vfs::VfsAction::ReadDir,
        })?)
        .send_and_await_response(test_timeout)??;

    let Message::Response { body: vfs_body, .. } = response else {
        fail!("tester");
    };
    let vfs::VfsResponse::ReadDir(mut children) =
        serde_json::from_slice(&vfs_body)?
    else {
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
        let grant_caps = caps_by_child
            .get(test_name)
            .and_then(|caps| {
                Some(
                    caps.iter()
                        .map(|cap| ProcessId::from_str(cap).unwrap())
                        .collect(),
                )
            })
            .unwrap_or(vec![]);
        let child_process_id = match spawn(
            None,
            &test_path,
            OnExit::None, //  TODO: notify us
            our_capabilities(),
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
        if let Err(FailResponse { test, file, line, column }) = result {
            fail!(test, file, line, column);
        }
    }

    println!("test_runner: done running {:?}", children);

    Response::new()
        .body(TesterResponse::Run(Ok(())))
        .send()?;

    Ok(())
}

fn handle_message(
    our: &Address,
    node_names: &mut Vec<String>,
) -> anyhow::Result<()> {
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
        match handle_message(&our, &mut node_names) {
            Ok(()) => {}
            Err(e) => {
                println!("tester: error: {:?}", e,);
                fail!("tester");
            }
        };
    }
}
