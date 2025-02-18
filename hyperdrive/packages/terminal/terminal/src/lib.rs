use crate::hyperware::process::terminal::{
    EditAliasResponse, Request as TerminalRequest, Response as TerminalResponse,
};
use hyperware_process_lib::{
    await_message, call_init, get_typed_state, kernel_types as kt, our_capabilities, println,
    set_state, vfs, Address, Capability, Message, ProcessId, Request, Response,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "target/wit",
    world: "terminal-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[derive(Debug, Serialize, Deserialize)]
enum ScriptError {
    UnknownName(String),
    FailedToReadWasm,
    NoScriptsManifest,
    NoScriptInManifest,
    InvalidScriptsManifest,
    KernelUnresponsive,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::UnknownName(name) => {
                write!(f, "'{name}' not found, either as an alias or process ID")
            }
            ScriptError::FailedToReadWasm => write!(f, "failed to read script Wasm from VFS"),
            ScriptError::NoScriptsManifest => write!(f, "no scripts manifest in package"),
            ScriptError::NoScriptInManifest => write!(f, "script not in scripts.json file"),
            ScriptError::InvalidScriptsManifest => write!(f, "could not parse scripts.json file"),
            ScriptError::KernelUnresponsive => write!(f, "kernel unresponsive"),
        }
    }
}

impl std::error::Error for ScriptError {}

#[derive(Serialize, Deserialize)]
#[serde(tag = "version")]
enum VersionedState {
    V1(TerminalStateV1),
}

#[derive(Serialize, Deserialize)]
struct TerminalStateV1 {
    our: Address,
    aliases: HashMap<String, ProcessId>,
}

impl VersionedState {
    /// Create a new terminal state with the default system aliases
    fn new(our: Address) -> Self {
        Self::V1(TerminalStateV1 {
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
                    "help".to_string(),
                    ProcessId::new(Some("help"), "terminal", "sys"),
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
                    "net-diagnostics".to_string(),
                    ProcessId::new(Some("net-diagnostics"), "terminal", "sys"),
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
        })
    }

    fn our(&self) -> &Address {
        match self {
            VersionedState::V1(state) => &state.our,
        }
    }

    fn aliases(&self) -> &HashMap<String, ProcessId> {
        match self {
            VersionedState::V1(state) => &state.aliases,
        }
    }

    fn alias_insert(&mut self, alias: String, process: ProcessId) {
        match self {
            VersionedState::V1(state) => {
                state.aliases.insert(alias, process);
            }
        }
    }

    fn alias_remove(&mut self, alias: &str) {
        match self {
            VersionedState::V1(state) => {
                state.aliases.remove(alias);
            }
        }
    }
}

call_init!(init);
fn init(our: Address) {
    let mut state: VersionedState =
        match get_typed_state(|bytes| bincode::deserialize::<VersionedState>(bytes)) {
            Some(mut s) => {
                // **add** the pre-installed scripts to the terminal state
                // in case new ones have been added or if user has deleted aliases
                let VersionedState::V1(default_state) = VersionedState::new(our);
                for (alias, process) in default_state.aliases {
                    s.alias_insert(alias, process);
                }
                s
            }
            None => VersionedState::new(our),
        };

    loop {
        let message = match await_message() {
            Err(e) => {
                println!("net error: {e:?}!");
                continue;
            }
            Ok(message) => message,
        };
        match message {
            Message::Request {
                source,
                body,
                expects_response,
                ..
            } => {
                // this is a message from the runtime terminal, parse as a command
                if *state.our() == source {
                    if let Err(e) =
                        parse_command(&mut state, String::from_utf8_lossy(&body).to_string())
                    {
                        println!("error calling script: {e}");
                    }
                // checks for a request from a terminal script (different process, same package)
                } else if state.our().node == source.node
                    && state.our().package() == source.package()
                {
                    let Ok(action) = serde_json::from_slice::<TerminalRequest>(&body) else {
                        println!("failed to parse TerminalRequest from {source}");
                        continue;
                    };
                    match action {
                        TerminalRequest::EditAlias(edit_alias_request) => {
                            let terminal_response = handle_alias_change(
                                &mut state,
                                edit_alias_request.alias,
                                edit_alias_request.process,
                            );
                            if expects_response.is_some() {
                                Response::new()
                                    .body(serde_json::to_vec(&terminal_response).unwrap())
                                    .send()
                                    .unwrap();
                            }
                        }
                    }
                } else {
                    hyperware_process_lib::print_to_terminal(
                        2,
                        &format!("ignoring message from {source}"),
                    );
                }
            }
            Message::Response { body, .. } => {
                if let Ok(txt) = std::str::from_utf8(&body) {
                    println!("{txt}");
                } else {
                    println!("{body:?}");
                }
            }
        }
    }
}

fn parse_command(state: &mut VersionedState, line: String) -> Result<(), ScriptError> {
    if line.is_empty() {
        return Ok(());
    }
    let (head, args) = line.split_once(" ").unwrap_or((&line, ""));
    match state.aliases().get(head) {
        Some(process) => handle_run(state.our(), process, args.to_string()),
        None => match head.parse::<ProcessId>() {
            Ok(pid) => handle_run(state.our(), &pid, args.to_string()),
            Err(_) => Err(ScriptError::UnknownName(head.to_string())),
        },
    }
}

/// Run a script by loading it from the VFS
fn handle_run(our: &Address, process: &ProcessId, args: String) -> Result<(), ScriptError> {
    let entry = get_entry(process)?;
    let wasm_path = format!(
        "/{}:{}/pkg/{}.wasm",
        process.package(),
        process.publisher(),
        process.process()
    );

    // all scripts are given random process IDs
    let process_id = ProcessId::new(None, process.package(), process.publisher());

    // call VFS manually so as not to fetch the blob, instead passing it to kernel request
    Request::to(("our", "vfs", "distro", "sys"))
        .body(
            serde_json::to_vec(&vfs::VfsRequest {
                path: wasm_path.clone(),
                action: vfs::VfsAction::Read,
            })
            .unwrap(),
        )
        .send_and_await_response(5)
        .unwrap()
        .map_err(|_| ScriptError::FailedToReadWasm)?;

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
            .body(
                serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                    target: process,
                    capabilities: vec![kt::de_wit_capability(cap)],
                })
                .unwrap(),
            )
            .send()
            .unwrap();
    }

    let mut requested_caps: HashSet<kt::Capability> = HashSet::new();
    if let Some(to_request) = &entry.request_capabilities {
        for value in to_request {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        requested_caps.insert(kt::Capability {
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
                                requested_caps.insert(kt::Capability {
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
    requested_caps.insert(kt::de_wit_capability(Capability {
        issuer: our.clone(),
        params: "\"messaging\"".to_string(),
    }));
    if entry.request_networking {
        requested_caps.insert(kt::de_wit_capability(Capability {
            issuer: Address::new(&our.node, ("kernel", "distro", "sys")),
            params: "\"network\"".to_string(),
        }));
    }
    if entry.root {
        for cap in our_capabilities() {
            requested_caps.insert(kt::de_wit_capability(cap));
        }
    }

    // inherits the blob from the previous request to VFS
    // containing the wasm byte code of the process
    Request::to(("our", "kernel", "distro", "sys"))
        .body(
            serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                id: process_id.clone(),
                wasm_bytes_handle: wasm_path,
                wit_version: entry.wit_version,
                on_exit: kt::OnExit::None,
                initial_capabilities: requested_caps,
                public: entry.public,
            })
            .unwrap(),
        )
        .inherit(true)
        .send_and_await_response(5)
        .unwrap()
        .map_err(|_| ScriptError::KernelUnresponsive)?;

    // run the process
    Request::to(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(process_id.clone())).unwrap())
        .send_and_await_response(5)
        .unwrap()
        .map_err(|_| ScriptError::KernelUnresponsive)?;

    // once process is running, send the arguments to it
    Request::to(("our", process_id))
        .body(args.into_bytes())
        .send()
        .unwrap();

    Ok(())
}

fn handle_alias_change(
    state: &mut VersionedState,
    alias: String,
    process: Option<String>,
) -> TerminalResponse {
    let response = match process {
        Some(process) => {
            let Ok(parsed_process) = process.parse::<ProcessId>() else {
                return TerminalResponse::EditAlias(EditAliasResponse::InvalidProcessId);
            };
            println!("alias {alias} set for {process}");
            state.alias_insert(alias, parsed_process);
            TerminalResponse::EditAlias(EditAliasResponse::AliasSet)
        }
        None => {
            if state.aliases().contains_key(&alias) {
                state.alias_remove(&alias);
                println!("alias {alias} removed");
                TerminalResponse::EditAlias(EditAliasResponse::AliasRemoved)
            } else {
                println!("alias {alias} not found");
                TerminalResponse::EditAlias(EditAliasResponse::AliasNotFound)
            }
        }
    };
    set_state(&bincode::serialize(&state).expect("failed to serialize terminal state"));
    response
}

fn get_entry(process: &ProcessId) -> Result<kt::DotScriptsEntry, ScriptError> {
    let file = vfs::File::new(
        format!(
            "/{}:{}/pkg/scripts.json",
            process.package(),
            process.publisher()
        ),
        5,
    )
    .read()
    .map_err(|_| ScriptError::NoScriptsManifest)?;

    let dot_scripts = serde_json::from_slice::<HashMap<String, kt::DotScriptsEntry>>(&file)
        .map_err(|_| ScriptError::InvalidScriptsManifest)?;
    let Some(entry) = dot_scripts.get(&format!("{}.wasm", process.process())) else {
        return Err(ScriptError::NoScriptInManifest);
    };
    Ok(entry.to_owned())
}
