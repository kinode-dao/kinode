use std::str::FromStr;

use kinode_process_lib::{
    await_message, call_init, our_capabilities, println, spawn, vfs,
    vfs::{DirEntry, FileType},
    Address, Message, OnExit, ProcessId, Request, Response,
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

fn make_vfs_address(our: &Address) -> anyhow::Result<Address> {
    Ok(Address::new(
        our.node.clone(),
        ProcessId::from_str("vfs:distro:sys")?,
    ))
}

fn handle_message(our: &Address) -> anyhow::Result<()> {
    let message = await_message()?;

    match message {
        Message::Response { .. } => {
            return Err(tt::TesterError::UnexpectedResponse.into());
        }
        Message::Request { ref body, .. } => {
            match serde_json::from_slice(body)? {
                tt::TesterRequest::Run {
                    ref test_names,
                    test_timeout,
                    ..
                } => {
                    println!("test_runner: got Run");

                    let dir_prefix = "tester:sys/tests";

                    let response = Request::new()
                        .target(make_vfs_address(&our)?)
                        .body(serde_json::to_vec(&vfs::VfsRequest {
                            path: dir_prefix.into(),
                            action: vfs::VfsAction::ReadDir,
                        })?)
                        .send_and_await_response(test_timeout)??;

                    let Message::Response { body: vfs_body, .. } = response else {
                        fail!("test_runner");
                    };
                    let vfs::VfsResponse::ReadDir(mut children) =
                        serde_json::from_slice(&vfs_body)?
                    else {
                        println!(
                            "{:?}",
                            serde_json::from_slice::<serde_json::Value>(&vfs_body)?
                        );
                        fail!("test_runner");
                    };

                    for test_name in test_names {
                        let test_entry = DirEntry {
                            path: format!("{}/{}.wasm", dir_prefix, test_name),
                            file_type: FileType::File,
                        };
                        if !children.contains(&test_entry) {
                            return Err(anyhow::anyhow!(
                                "test {} not found amongst {:?}",
                                test_name,
                                children,
                            ));
                        }
                    }

                    let caps_file_path = format!("{}/grant_capabilities.json", dir_prefix);
                    let caps_index = children.iter().position(|i| *i.path == *caps_file_path);
                    let caps_by_child: std::collections::HashMap<String, Vec<String>> =
                        match caps_index {
                            None => std::collections::HashMap::new(),
                            Some(caps_index) => {
                                children.remove(caps_index);
                                let file = vfs::file::open_file(&caps_file_path, false, None)?;
                                let file_contents = file.read()?;
                                serde_json::from_slice(&file_contents)?
                            }
                        };

                    println!("test_runner: running {:?}...", children);

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
                                fail!("test_runner");
                            }
                        };

                        let response = Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })
                            .body(body.clone())
                            .send_and_await_response(test_timeout)??;

                        let Message::Response { body, .. } = response else {
                            fail!("test_runner");
                        };
                        match serde_json::from_slice(&body)? {
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

                    println!("test_runner: done running {:?}", children);

                    Response::new()
                        .body(serde_json::to_vec(&tt::TesterResponse::Pass)?)
                        .send()?;
                }
                tt::TesterRequest::KernelMessage(_) | tt::TesterRequest::GetFullMessage(_) => {
                    fail!("test_runner");
                }
            }
            Ok(())
        }
    }
}

call_init!(init);
fn init(our: Address) {
    println!("{}: begin", our);

    loop {
        match handle_message(&our) {
            Ok(()) => {}
            Err(e) => {
                println!("test_runner: error: {:?}", e);
                fail!("test_runner");
            }
        };
    }
}
