use anyhow::anyhow;
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::kinode::process::standard as wit;
use kinode_process_lib::{
    call_init, get_blob, get_typed_state, our_capabilities, print_to_terminal, println, set_state,
    vfs, Address, Capability, ProcessId, Request,
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

    match handle_run(&state.our, &process, args.to_string()) {
        Ok(_) => Ok(()), // TODO clean up process
        Err(e) => Err(anyhow!("failed to instantiate script: {}", e)),
    }
}

call_init!(init);
fn init(our: Address) {
    let mut state: TerminalState = match get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)) {
        Some(s) => s,
        None => TerminalState {
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
        },
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
                } else if state.our.node == source.node && state.our.package() == source.package() {
                    let Ok(action) = serde_json::from_slice::<TerminalAction>(&body) else {
                        println!("failed to parse action from: {}", source);
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
                    println!("ignoring message from: {}", source);
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
    let wasm_path = format!("{}.wasm", process.process());
    let package = format!("{}:{}", process.package(), process.publisher());
    let drive_path = format!("/{}/pkg", package);
    let Ok(entry) = get_entry(process) else {
        return Err(anyhow::anyhow!("script not in scripts.json file"));
    };
    let wasm_path = if wasm_path.starts_with("/") {
        wasm_path
    } else {
        format!("/{}", wasm_path)
    };
    let wasm_path = format!("{}{}", drive_path, wasm_path);
    // build initial caps
    let process_id = format!("{}:{}", rand::random::<u64>(), package); // all scripts are given random process IDs
    let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
        return Err(anyhow::anyhow!("invalid process id!"));
    };

    let _bytes_response = Request::new()
        .target(("our", "vfs", "distro", "sys"))
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
                                    process: parsed_new_process_id.clone(),
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
                                            process: parsed_new_process_id.clone(),
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
        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                target: process,
                capabilities: vec![kt::de_wit_capability(cap)],
            })?)
            .send()?;
    }
    Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
            id: parsed_new_process_id.clone(),
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
    print_to_terminal(
        3,
        &format!(
            "{}: Process {{\n    wasm_bytes_handle: {},\n    on_exit: {:?},\n    public: {}\n    capabilities: {}\n}}",
            parsed_new_process_id.clone(),
            wasm_path.clone(),
            kt::OnExit::None,
            entry.public,
            {
                let mut caps_string = "[".to_string();
                for cap in requested_caps.iter() {
                    caps_string += &format!("\n        {}({})", cap.issuer.to_string(), cap.params);
                }
                caps_string + "\n    ]"
            },
        ),
    );
    Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
            target: parsed_new_process_id.clone(),
            capabilities: requested_caps,
        })?)
        .send()?;
    let _ = Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
            parsed_new_process_id.clone(),
        ))?)
        .send_and_await_response(5)??;
    let req = Request::new()
        .target(("our", parsed_new_process_id))
        .body(args.into_bytes());

    req.send().unwrap();

    Ok(())
}

fn handle_alias_change(
    state: &mut TerminalState,
    alias: String,
    process: Option<ProcessId>,
) -> anyhow::Result<()> {
    match process {
        Some(process) => {
            // first check to make sure the script is actually a script
            let Ok(_) = get_entry(&process) else {
                return Err(anyhow!("process {} not found", process));
            };

            state.aliases.insert(alias.clone(), process.clone());
            println!("alias {} set to {}", alias, process);
        }
        None => {
            if state.aliases.contains_key(&alias) {
                state.aliases.remove(&alias);
                println!("alias {} removed", alias);
            } else {
                println!("alias {} not found", alias);
            }
        }
    }
    set_state(&bincode::serialize(&state)?);
    Ok(())
}

fn get_entry(process: &ProcessId) -> anyhow::Result<kt::DotScriptsEntry> {
    let drive_path = format!("/{}:{}/pkg", process.package(), process.publisher());
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/scripts.json", drive_path),
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
