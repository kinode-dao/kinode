use anyhow::anyhow;
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::kinode::process::standard as wit;
use kinode_process_lib::{
    call_init, get_blob, get_typed_state, our_capabilities, println, set_state, vfs, Address,
    Capability, ProcessId, Request,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

#[derive(Debug, Serialize, Deserialize)]
enum TerminalAction {
    EditAlias {
        alias: String,
        process: Option<ProcessId>,
    },
}

#[derive(Serialize, Deserialize)]
struct TerminalState {
    our: Address,
    aliases: HashMap<String, ProcessId>,
}

fn parse_command(state: &mut TerminalState, line: &str) -> anyhow::Result<()> {
    if line.is_empty() {
        return Ok(());
    }
    let (head, args) = line.split_once(" ").unwrap_or((line, ""));
    let process = match state.aliases.get(head) {
        Some(pid) => pid.clone(),
        None => match head.parse::<ProcessId>() {
            Ok(pid) => pid,
            Err(_) => {
                return Err(anyhow!("invalid script name"));
            }
        },
    };

    handle_run(&state.our, &process, args.to_string())
}

call_init!(init);
fn init(our: Address) {
    let mut state: TerminalState = match get_typed_state(|bytes| bincode::deserialize(bytes)) {
        Some(s) => s,
        None => {
            let state = TerminalState {
                our,
                aliases: HashMap::from([
                    (
                        "alias".to_string(),
                        ProcessId::new(Some("alias"), "terminal", "sys"),
                    ),
                    (
                        "cat".to_string(),
                        ProcessId::new(Some("cat"), "terminal", "sys"),
                    ),
                    (
                        "echo".to_string(),
                        ProcessId::new(Some("echo"), "terminal", "sys"),
                    ),
                    (
                        "hi".to_string(),
                        ProcessId::new(Some("hi"), "terminal", "sys"),
                    ),
                    (
                        "kill".to_string(),
                        ProcessId::new(Some("kill"), "terminal", "sys"),
                    ),
                    (
                        "kfetch".to_string(),
                        ProcessId::new(Some("kfetch"), "terminal", "sys"),
                    ),
                    (
                        "m".to_string(),
                        ProcessId::new(Some("m"), "terminal", "sys"),
                    ),
                    (
                        "namehash_to_name".to_string(),
                        ProcessId::new(Some("namehash_to_name"), "terminal", "sys"),
                    ),
                    (
                        "net_diagnostics".to_string(),
                        ProcessId::new(Some("net_diagnostics"), "terminal", "sys"),
                    ),
                    (
                        "peer".to_string(),
                        ProcessId::new(Some("peer"), "terminal", "sys"),
                    ),
                    (
                        "peers".to_string(),
                        ProcessId::new(Some("peers"), "terminal", "sys"),
                    ),
                    (
                        "top".to_string(),
                        ProcessId::new(Some("top"), "terminal", "sys"),
                    ),
                ]),
            };
            set_state(&bincode::serialize(&state).unwrap());
            state
        }
    };

    loop {
        let (source, message) = match wit::receive() {
            Ok((source, message)) => (source, message),
            Err((error, _context)) => {
                println!("net error: {:?}!", error.kind);
                continue;
            }
        };
        match message {
            wit::Message::Request(wit::Request { body, .. }) => {
                if state.our == source {
                    match parse_command(&mut state, std::str::from_utf8(&body).unwrap_or_default())
                    {
                        Ok(()) => continue,
                        Err(e) => println!("{e}"),
                    }
                // checks for a request from a terminal script (different process, same package)
                } else if state.our.node == source.node && state.our.package() == source.package() {
                    let Ok(action) = serde_json::from_slice::<TerminalAction>(&body) else {
                        println!("failed to parse action from {source}");
                        continue;
                    };
                    match action {
                        TerminalAction::EditAlias { alias, process } => {
                            match handle_alias_change(&mut state, alias, process) {
                                Ok(()) => continue,
                                Err(e) => println!("{e}"),
                            };
                        }
                    }
                } else {
                    println!("ignoring message from {source}");
                    continue;
                }
            }
            wit::Message::Response((wit::Response { body, .. }, _)) => {
                if let Ok(txt) = std::str::from_utf8(&body) {
                    println!("{txt}");
                } else {
                    println!("{body:?}");
                }
            }
        }
    }
}

fn handle_run(our: &Address, process: &ProcessId, args: String) -> anyhow::Result<()> {
    let drive_path = format!("/{}:{}/pkg", process.package(), process.publisher());
    let Ok(entry) = get_entry(process) else {
        return Err(anyhow::anyhow!("script not in scripts.json file"));
    };
    let wasm_path = format!("{drive_path}/{}.wasm", process.process());

    // all scripts are given random process IDs
    let process_id = ProcessId::new(None, process.package(), process.publisher());

    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: wasm_path.clone(),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    // process the caps we are going to grant to other processes
    let mut granted_caps: Vec<(ProcessId, Capability)> = vec![];
    if let Some(to_grant) = &entry.grant_capabilities {
        for value in to_grant {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        granted_caps.push((
                            parsed_process_id,
                            Capability {
                                issuer: Address {
                                    node: our.node.clone(),
                                    process: process_id.clone(),
                                },
                                params: "\"messaging\"".into(),
                            },
                        ));
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
                                granted_caps.push((
                                    parsed_process_id,
                                    Capability {
                                        issuer: Address {
                                            node: our.node.clone(),
                                            process: process_id.clone(),
                                        },
                                        params: params.to_string(),
                                    },
                                ));
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
    for (process, cap) in granted_caps.into_iter() {
        Request::to(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                target: process,
                capabilities: vec![kt::de_wit_capability(cap)],
            })?)
            .send()?;
    }
    // inherits the blob from the previous request, `_bytes_response`,
    // containing the wasm byte code of the process
    Request::to(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
            id: process_id.clone(),
            wasm_bytes_handle: wasm_path.clone(),
            wit_version: entry.wit_version,
            on_exit: kt::OnExit::None,
            initial_capabilities: HashSet::new(),
            public: entry.public,
        })?)
        .inherit(true)
        .send_and_await_response(5)??;
    let mut requested_caps: Vec<kt::Capability> = vec![];
    if let Some(to_request) = &entry.request_capabilities {
        for value in to_request {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        requested_caps.push(kt::Capability {
                            issuer: Address {
                                node: our.node.clone(),
                                process: parsed_process_id.clone(),
                            },
                            params: "\"messaging\"".into(),
                        });
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
                                requested_caps.push(kt::Capability {
                                    issuer: Address {
                                        node: our.node.clone(),
                                        process: parsed_process_id.clone(),
                                    },
                                    params: params.to_string(),
                                });
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
    // always give it the cap to message the terminal back
    requested_caps.push(kt::de_wit_capability(Capability {
        issuer: our.clone(),
        params: "\"messaging\"".to_string(),
    }));
    if entry.request_networking {
        requested_caps.push(kt::de_wit_capability(Capability {
            issuer: Address::new(&our.node, ("kernel", "distro", "sys")),
            params: "\"network\"".to_string(),
        }));
    }
    if entry.root {
        for cap in our_capabilities() {
            requested_caps.push(kt::de_wit_capability(cap.clone()));
        }
    }
    Request::to(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
            target: process_id.clone(),
            capabilities: requested_caps,
        })?)
        .send()?;
    Request::to(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
            process_id.clone(),
        ))?)
        .send_and_await_response(5)??;
    Request::to(("our", process_id))
        .body(args.into_bytes())
        .send()?;
    Ok(())
}

fn handle_alias_change(
    state: &mut TerminalState,
    alias: String,
    process: Option<ProcessId>,
) -> anyhow::Result<()> {
    match process {
        Some(process) => {
            println!("alias {alias} set for {process}");
            state.aliases.insert(alias, process);
        }
        None => {
            if state.aliases.contains_key(&alias) {
                state.aliases.remove(&alias);
                println!("alias {alias} removed");
            } else {
                println!("alias {alias} not found");
            }
        }
    }
    set_state(&bincode::serialize(&state)?);
    Ok(())
}

fn get_entry(process: &ProcessId) -> anyhow::Result<kt::DotScriptsEntry> {
    let drive_path = format!("/{}:{}/pkg", process.package(), process.publisher());
    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{drive_path}/scripts.json"),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!(
            "couldn't find /{}/pkg/scripts.json",
            process.package()
        ));
    };
    let dot_scripts = String::from_utf8(blob.bytes)?;
    let dot_scripts = serde_json::from_str::<HashMap<String, kt::DotScriptsEntry>>(&dot_scripts)?;
    let Some(entry) = dot_scripts.get(&format!("{}.wasm", process.process())) else {
        return Err(anyhow::anyhow!("script not in scripts.json file"));
    };
    Ok(entry.clone())
}
