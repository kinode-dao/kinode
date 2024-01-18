use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use kinode_process_lib::{
    await_message, call_init, println, Address, Capability, Message, ProcessId, Request, Response,
};

// TODO move this into kt::
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DotScriptsEntry {
    pub public: bool,
    pub request_networking: bool,
    pub request_capabilities: Option<Vec<serde_json::Value>>,
    pub grant_capabilities: Option<Vec<serde_json::Value>>,
}

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
pub enum ScriptRequest {
    Run {
        package: PackageId,
        wasm_path: String, // vfs path
        args: String,      // first message, in json
    },
    Inject {
        process: String, // ProcessId
        args: String,    // next message, in json
    },
    Terminate(String), // ProcessId string encoded
}

call_init!(init);
fn init(our: Address) {
    println!("runner:script : begin");

    loop {
        match handle_message(&our) {
            Ok(()) => {}
            Err(e) => {
                println!("script_runner: error: {:?}", e);
            }
        };
    }
}

fn handle_message(our: &Address) -> anyhow::Result<()> {
    let message = await_message()?;
    match message {
        Message::Response { .. } => {
            return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
        }
        Message::Request {
            ref source,
            ref body,
            ..
        } => match serde_json::from_slice::<ScriptRequest>(body)? {
            ScriptRequest::Run {
                package,
                wasm_path,
                args,
            } => match handle_run(our, &package, wasm_path, args) {
                Ok(()) => {} // LocalResponse::InstallResponse(InstallResponse::Success),
                Err(_) => {} // LocalResponse::InstallResponse(InstallResponse::Failure),
            },
            ScriptRequest::Inject { process, args } => {
                println!("script_runner: got inject request");
            }
            ScriptRequest::Terminate(process) => {
                println!("script_runner: got terminate request");
            }
        },
    }
    Ok(())
}

fn handle_run(
    our: &Address,
    package: &PackageId,
    wasm_path: String,
    args: String,
) -> anyhow::Result<()> {
    let drive_path = format!("/{}/pkg", package);
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/scripts.json", drive_path),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let dot_scripts = String::from_utf8(blob.bytes)?;
    let dot_scripts = serde_json::from_str::<HashMap<String, DotScriptsEntry>>(&dot_scripts)?;
    let Some(entry) = dot_scripts.get(&wasm_path) else {
        return Err(anyhow::anyhow!("script not in scripts.json file"));
    };
    let wasm_path = if wasm_path.starts_with("/") {
        wasm_path.clone()
    } else {
        format!("/{}", wasm_path)
    };
    let wasm_path = format!("{}{}", drive_path, wasm_path);
    // build initial caps
    let mut initial_capabilities: HashSet<kt::Capability> = HashSet::new();
    if entry.request_networking {
        initial_capabilities.insert(kt::de_wit_capability(Capability {
            issuer: Address::new(&our.node, ("kernel", "distro", "sys")),
            params: "\"network\"".to_string(),
        }));
    }
    let process_id = format!("{}:{}", rand::random::<u64>(), package); // all scripts are given random process IDs
    let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
        return Err(anyhow::anyhow!("app store: invalid process id!"));
    };

    let _bytes_response = Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: wasm_path.clone(),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    if let Some(to_request) = &entry.request_capabilities {
        for value in to_request {
            let mut capability = None;
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        capability = get_capability(
                            &Address {
                                node: our.node.clone(),
                                process: parsed_process_id.clone(),
                            },
                            "\"messaging\"".into(),
                        );
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                capability = get_capability(
                                    &Address {
                                        node: our.node.clone(),
                                        process: parsed_process_id.clone(),
                                    },
                                    &params.to_string(),
                                );
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
            if let Some(cap) = capability {
                initial_capabilities.insert(kt::de_wit_capability(cap));
            } else {
                println!(
                    "runner: no cap: {}, for {} to request!",
                    value.to_string(),
                    package
                );
            }
        }
    }
    Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
            id: parsed_new_process_id.clone(),
            wasm_bytes_handle: wasm_path,
            wit_version: None,
            on_exit: kt::OnExit::None, // TODO this should send a message back to runner:script:sys so that it can Drop capabilities
            initial_capabilities,
            public: entry.public,
        })?)
        .inherit(true)
        .send_and_await_response(5)??;
    if let Some(to_grant) = &entry.grant_capabilities {
        for value in to_grant {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        let _ = Request::new()
                            .target(("our", "kernel", "distro", "sys"))
                            .body(
                                serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                                    target: parsed_process_id,
                                    capabilities: vec![kt::Capability {
                                        issuer: Address {
                                            node: our.node.clone(),
                                            process: parsed_new_process_id.clone(),
                                        },
                                        params: "\"messaging\"".into(),
                                    }],
                                })
                                .unwrap(),
                            )
                            .send()?;
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                let _ = Request::new()
                                    .target(("our", "kernel", "distro", "sys"))
                                    .body(
                                        serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                                            target: parsed_process_id,
                                            capabilities: vec![kt::Capability {
                                                issuer: Address {
                                                    node: our.node.clone(),
                                                    process: parsed_new_process_id.clone(),
                                                },
                                                params: params.to_string(),
                                            }],
                                        })
                                        .unwrap(),
                                    )
                                    .send()?;
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
        }
    }
    let _ = Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
            parsed_new_process_id.clone(),
        ))?)
        .send_and_await_response(5)??;
    let _ = Request::new()
        .target(("our", parsed_new_process_id))
        .body(args.into_bytes())
        .send();
    Ok(())
}
