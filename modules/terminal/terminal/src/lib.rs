use anyhow::anyhow;
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::kinode::process::standard as wit;
use kinode_process_lib::{
    get_blob, get_capability, get_typed_state, our_capabilities, println, set_state, vfs, Address,
    Capability, PackageId, ProcessId, Request,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
struct EditAliases {
    alias: String,
    process: Option<ProcessId>,
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

    let re = Regex::new(r"(.*?)\|(\d+)\s*(.*)").unwrap();
    let pipe = match re.captures(args) {
        Some(caps) => {
            let parsed_args = caps
                .get(1)
                .map_or("", |m| m.as_str())
                .trim_end()
                .to_string();

            let time_str = caps.get(2).map_or("", |m| m.as_str());
            let time: u64 = time_str.parse().unwrap_or(0);

            let pipe = caps
                .get(3)
                .map_or("", |m| m.as_str())
                .trim_start()
                .to_string();

            (parsed_args, Some((pipe, time)))
        }
        None => (args.to_string(), None),
    };

    let wasm_path = format!("{}.wasm", process.process());
    let package = PackageId::new(process.package(), process.publisher());
    match handle_run(&state.our, &package, wasm_path, pipe.0, pipe.1) {
        Ok(_) => Ok(()), // TODO clean up process
        Err(e) => Err(anyhow!("failed to instantiate script: {}", e)),
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let mut state: TerminalState =
            match get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)) {
                Some(s) => s,
                None => TerminalState {
                    our: our.parse::<Address>().unwrap(),
                    aliases: HashMap::from([
                        (
                            "alias".to_string(),
                            "alias:terminal:sys".parse::<ProcessId>().unwrap(),
                        ),
                        (
                            "cat".to_string(),
                            "cat:terminal:sys".parse::<ProcessId>().unwrap(),
                        ),
                        (
                            "echo".to_string(),
                            "echo:terminal:sys".parse::<ProcessId>().unwrap(),
                        ),
                        (
                            "hi".to_string(),
                            "hi:terminal:sys".parse::<ProcessId>().unwrap(),
                        ),
                        (
                            "m".to_string(),
                            "m:terminal:sys".parse::<ProcessId>().unwrap(),
                        ),
                        (
                            "top".to_string(),
                            "top:terminal:sys".parse::<ProcessId>().unwrap(),
                        ),
                    ]),
                },
            };

        loop {
            let (source, message) = match wit::receive() {
                Ok((source, message)) => (source, message),
                Err((error, _context)) => {
                    println!("terminal: net error: {:?}!", error.kind);
                    continue;
                }
            };
            match message {
                wit::Message::Request(wit::Request { body, .. }) => {
                    if state.our == source {
                        match parse_command(
                            &mut state,
                            std::str::from_utf8(&body).unwrap_or_default(),
                        ) {
                            Ok(()) => continue,
                            Err(e) => println!("terminal: {e}"),
                        }
                    } else if state.our.node == source.node {
                        let Ok(edit_aliases) = serde_json::from_slice::<EditAliases>(&body) else {
                            println!("terminal: invalid action!");
                            continue;
                        };

                        match edit_aliases.process {
                            Some(process) => {
                                state
                                    .aliases
                                    .insert(edit_aliases.alias.clone(), process.clone());
                                println!(
                                    "terminal: alias {} set to {}",
                                    edit_aliases.alias, process
                                );
                            }
                            None => {
                                state.aliases.remove(&edit_aliases.alias);
                                println!("terminal: alias {} removed", edit_aliases.alias);
                            }
                        }
                        if let Ok(new_state) = bincode::serialize(&state) {
                            set_state(&new_state);
                        } else {
                            println!("terminal: failed to serialize state!");
                        }
                    } else {
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
}

fn handle_run(
    our: &Address,
    package: &PackageId,
    wasm_path: String,
    args: String,
    pipe: Option<(String, u64)>,
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
        return Err(anyhow::anyhow!(
            "couldn't find /{}/pkg/scripts.json",
            package
        ));
    };
    let dot_scripts = String::from_utf8(blob.bytes)?;
    let dot_scripts = serde_json::from_str::<HashMap<String, kt::DotScriptsEntry>>(&dot_scripts)?;
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
            initial_capabilities: if entry.root {
                our_capabilities()
                    .iter()
                    .map(|wit: &kinode_process_lib::Capability| kt::de_wit_capability(wit.clone()))
                    .collect()
            } else {
                initial_capabilities
            },
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
    let req = Request::new()
        .target(("our", parsed_new_process_id))
        .body(args.into_bytes());

    let Some(pipe) = pipe else {
        req.send().unwrap();
        return Ok(());
    };

    let Ok(res) = req.clone().send_and_await_response(pipe.1).unwrap() else {
        return Err(anyhow::anyhow!("script timed out"));
    };

    let _ = Request::new()
        .target(our)
        .body(
            format!(
                "{} {}",
                pipe.0,
                String::from_utf8(res.body().to_vec()).unwrap()
            )
            .into_bytes()
            .to_vec(),
        )
        .send()?;

    Ok(())
}
