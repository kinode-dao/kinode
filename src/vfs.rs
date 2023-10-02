use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::types::*;

const VFS_PERSIST_STATE_CHANNEL_CAPACITY: usize = 5;
const VFS_TASK_DONE_CHANNEL_CAPACITY: usize = 5;
const VFS_RESPONSE_CHANNEL_CAPACITY: usize = 2;

type ResponseRouter = HashMap<u64, MessageSender>;
#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
enum Key {
    Dir { id: u64 },
    File { id: u128 },
    // ...
}
type KeyToEntry = HashMap<Key, Entry>;
type PathToKey = HashMap<String, Key>;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Vfs {
    key_to_entry: KeyToEntry,
    path_to_key: PathToKey,
}
type IdentifierToVfs = HashMap<String, Arc<Mutex<Vfs>>>;
type IdentifierToVfsSerializable = HashMap<String, Vfs>;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Entry {
    name: String,
    full_path: String, //  full_path, ending with `/` for dir
    entry_type: EntryType,
    // ...  //  general metadata?
}
#[derive(Clone, Debug, Deserialize, Serialize)]
enum EntryType {
    Dir { parent: Key, children: HashSet<Key> },
    File { parent: Key }, //  hash could be generalized to `location` if we want to be able to point at, e.g., remote files
                          // ...  //  symlinks?
}

impl Vfs {
    fn new() -> Self {
        let mut key_to_entry: KeyToEntry = HashMap::new();
        let mut path_to_key: PathToKey = HashMap::new();
        let root_path: String = "/".into();
        let root_key = Key::Dir { id: 0 };
        key_to_entry.insert(
            root_key.clone(),
            Entry {
                name: root_path.clone(),
                full_path: root_path.clone(),
                entry_type: EntryType::Dir {
                    parent: root_key.clone(),
                    children: HashSet::new(),
                },
            },
        );
        path_to_key.insert(root_path.clone(), root_key.clone());
        Vfs {
            key_to_entry,
            path_to_key,
        }
    }
}

fn make_dir_name(full_path: &str) -> (String, String) {
    if full_path == "/" {
        return ("/".into(), "".into()); //  root case
    }
    let mut split_path: Vec<&str> = full_path.split("/").collect();
    let _ = split_path.pop();
    let name = format!("{}/", split_path.pop().unwrap());
    let path = split_path.join("/");
    let path = if path == "" {
        "/".into()
    } else {
        format!("{}/", path)
    };
    (name, path)
}

fn make_file_name(full_path: &str) -> (String, String) {
    let mut split_path: Vec<&str> = full_path.split("/").collect();
    let name = split_path.pop().unwrap();
    let path = format!("{}/", split_path.join("/"));
    (name.into(), path)
}

fn make_error_message(
    our_name: String,
    id: u64,
    source: Address,
    error: VfsError,
) -> KernelMessage {
    KernelMessage {
        id,
        source: Address {
            node: our_name,
            process: ProcessId::Name("vfs".into()),
        },
        target: source,
        rsvp: None,
        message: Message::Response((
            Response {
                ipc: Some(serde_json::to_string(&error).unwrap()), //  TODO: handle error?
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}

async fn state_to_bytes(state: &IdentifierToVfs) -> Vec<u8> {
    let mut serializable: IdentifierToVfsSerializable = HashMap::new();
    for (id, vfs) in state.iter() {
        let vfs = vfs.lock().await;
        serializable.insert(id.clone(), (*vfs).clone());
    }
    bincode::serialize(&serializable).unwrap()
}

fn bytes_to_state(bytes: &Vec<u8>, state: &mut IdentifierToVfs) {
    let serializable: IdentifierToVfsSerializable = bincode::deserialize(&bytes).unwrap();
    for (id, vfs) in serializable.into_iter() {
        state.insert(id, Arc::new(Mutex::new(vfs)));
    }
}

async fn persist_state(our_node: String, send_to_loop: &MessageSender, state: &IdentifierToVfs) {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our_node.clone(),
                process: ProcessId::Name("vfs".into()),
            },
            target: Address {
                node: our_node,
                process: ProcessId::Name("filesystem".into()),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: true,
                expects_response: Some(5), // TODO evaluate
                ipc: Some(serde_json::to_string(&FsAction::SetState).unwrap()),
                metadata: None,
            }),
            payload: Some(Payload {
                mime: None,
                bytes: state_to_bytes(state).await,
            }),
            signed_capabilities: None,
        })
        .await;
}

async fn load_state_from_reboot(
    our_node: String,
    send_to_loop: &MessageSender,
    recv_from_loop: &mut MessageReceiver,
    identifier_to_vfs: &mut IdentifierToVfs,
) -> bool {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our_node.clone(),
                process: ProcessId::Name("vfs".into()),
            },
            target: Address {
                node: our_node.clone(),
                process: ProcessId::Name("filesystem".into()),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: true,
                expects_response: Some(5), // TODO evaluate
                ipc: Some(serde_json::to_string(&FsAction::GetState).unwrap()),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        })
        .await;
    let km = recv_from_loop.recv().await;
    let Some(km) = km else {
        return false;
    };

    let KernelMessage {
        message, payload, ..
    } = km;
    let Message::Response((Response { ipc, metadata: _ }, None)) = message else {
        return false;
    };
    let Ok(Ok(FsResponse::GetState)) =
        serde_json::from_str::<Result<FsResponse, FileSystemError>>(&ipc.unwrap_or_default())
    else {
        return false;
    };
    let Some(payload) = payload else {
        panic!("");
    };
    bytes_to_state(&payload.bytes, identifier_to_vfs);

    return true;
}

fn build_state_for_initial_boot(process_map: &ProcessMap, identifier_to_vfs: &mut IdentifierToVfs) {
    //  add wasm bytes to each process' vfs and to terminal's vfs
    let mut terminal_vfs = Vfs::new();
    for (process_id, persisted) in process_map.iter() {
        let mut vfs = Vfs::new();
        let ProcessId::Name(id) = process_id else {
            println!("vfs: initial boot skip adding bytes for {:?}", process_id);
            continue;
        };
        let name = format!("{}.wasm", id);
        let full_path = format!("/{}", name);
        let key = Key::File {
            id: persisted.wasm_bytes_handle.clone(),
        };
        let entry_type = EntryType::File {
            parent: Key::Dir { id: 0 },
        };
        let entry = Entry {
            name,
            full_path: full_path.clone(),
            entry_type,
        };
        vfs.key_to_entry.insert(key.clone(), entry.clone());
        vfs.path_to_key.insert(full_path.clone(), key.clone());
        identifier_to_vfs.insert(id.clone(), Arc::new(Mutex::new(vfs)));

        terminal_vfs.key_to_entry.insert(key.clone(), entry);
        terminal_vfs.path_to_key.insert(full_path.clone(), key);
    }
    identifier_to_vfs.insert("terminal".into(), Arc::new(Mutex::new(terminal_vfs)));

    //  initial caps are given to processes in src/filesystem/mod.rs:bootstrap()
}

pub async fn vfs(
    our_node: String,
    process_map: ProcessMap,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
) -> anyhow::Result<()> {
    let mut identifier_to_vfs: IdentifierToVfs = HashMap::new();
    let mut response_router: ResponseRouter = HashMap::new();
    let (send_vfs_task_done, mut recv_vfs_task_done): (
        tokio::sync::mpsc::Sender<u64>,
        tokio::sync::mpsc::Receiver<u64>,
    ) = tokio::sync::mpsc::channel(VFS_TASK_DONE_CHANNEL_CAPACITY);
    let (send_persist_state, mut recv_persist_state): (
        tokio::sync::mpsc::Sender<bool>,
        tokio::sync::mpsc::Receiver<bool>,
    ) = tokio::sync::mpsc::channel(VFS_PERSIST_STATE_CHANNEL_CAPACITY);

    let is_reboot = load_state_from_reboot(
        our_node.clone(),
        &send_to_loop,
        &mut recv_from_loop,
        &mut identifier_to_vfs,
    )
    .await;
    if !is_reboot {
        //  initial boot
        build_state_for_initial_boot(&process_map, &mut identifier_to_vfs);
        send_persist_state.send(true).await.unwrap();
    }

    loop {
        tokio::select! {
            id_done = recv_vfs_task_done.recv() => {
                let Some(id_done) = id_done else { continue };
                response_router.remove(&id_done);
            },
            _ = recv_persist_state.recv() => {
                persist_state(our_node.clone(), &send_to_loop, &identifier_to_vfs).await;
                continue;
            },
            km = recv_from_loop.recv() => {
                let Some(km) = km else { continue };
                if let Some(response_sender) = response_router.get(&km.id) {
                    response_sender.send(km).await.unwrap();
                    continue;
                }

                let KernelMessage {
                    id,
                    source,
                    rsvp,
                    message,
                    payload,
                    ..
                } = km;
                let Message::Request(Request {
                    expects_response,
                    ipc: Some(ipc),
                    metadata, // we return this to Requester for kernel reasons
                    ..
                }) = message.clone()
                else {
                    //  println!("vfs: {}", message);
                    continue;
                    // return Err(FileSystemError::BadJson {
                    //     json: "".into(),
                    //     error: "not a Request with payload".into(),
                    // });
                };

                let request: VfsRequest = match serde_json::from_str(&ipc) {
                    Ok(r) => r,
                    Err(e) => {
                        panic!("{}", e);
                        // return Err(FileSystemError::BadJson {
                        //     json: ipc.into(),
                        //     error: format!("parse failed: {:?}", e),
                        // })
                    }
                };

                if our_node != source.node {
                    println!(
                        "vfs: request must come from our_node={}, got: {}",
                        our_node,
                        source.node,
                    );
                    continue;
                }

                let (identifier, is_new) = match &request {
                    VfsRequest::New { identifier } => (identifier.clone(), true),
                    VfsRequest::Add { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::Rename { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::Delete { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::WriteOffset { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::GetPath { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::GetEntry { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::GetFileChunk { identifier, .. } => (identifier.clone(), false),
                    VfsRequest::GetEntryLength { identifier, .. } => (identifier.clone(), false),
                };

                let (vfs, new_caps) = match identifier_to_vfs.get(&identifier) {
                    Some(vfs) => (Arc::clone(vfs), vec![]),
                    None => {
                        if !is_new {
                            println!("vfs: invalid Request: non-New to non-existent");
                            send_to_loop
                                .send(make_error_message(
                                    our_node.clone(),
                                    id,
                                    source.clone(),
                                    VfsError::BadIdentifier,
                                ))
                                .await
                                .unwrap();
                            continue;
                        }
                        identifier_to_vfs.insert(
                            identifier.clone(),
                            Arc::new(Mutex::new(Vfs::new())),
                        );
                        let read_cap = Capability {
                            issuer: Address {
                                node: our_node.clone(),
                                process: ProcessId::Name("vfs".into()),
                            },
                            params: serde_json::to_string(&serde_json::json!({"kind": "read", "identifier": identifier})).unwrap(),
                        };
                        let write_cap = Capability {
                            issuer: Address {
                                node: our_node.clone(),
                                process: ProcessId::Name("vfs".into()),
                            },
                            params: serde_json::to_string(&serde_json::json!({"kind": "write", "identifier": identifier})).unwrap(),
                        };
                        (
                            Arc::clone(identifier_to_vfs.get(&identifier).unwrap()),
                            vec![read_cap, write_cap],
                        )
                    }
                };

                //  TODO: remove after vfs is stable
                let _ = send_to_terminal.send(Printout {
                    verbosity: 1,
                    content: format!("{:?}", vfs)
                }).await;

                let (response_sender, response_receiver): (
                    MessageSender,
                    MessageReceiver,
                ) = tokio::sync::mpsc::channel(VFS_RESPONSE_CHANNEL_CAPACITY);
                response_router.insert(id.clone(), response_sender);
                let our_node = our_node.clone();
                let send_to_loop = send_to_loop.clone();
                let send_persist_state = send_persist_state.clone();
                let send_to_terminal = send_to_terminal.clone();
                let send_to_caps_oracle = send_to_caps_oracle.clone();
                let send_vfs_task_done = send_vfs_task_done.clone();
                match &message {
                    Message::Response(_) => {},
                    Message::Request(_) => {
                        tokio::spawn(async move {
                            match handle_request(
                                our_node.clone(),
                                id,
                                source.clone(),
                                expects_response,
                                rsvp,
                                request,
                                metadata,
                                payload,
                                new_caps,
                                vfs,
                                send_to_loop.clone(),
                                send_persist_state,
                                send_to_terminal,
                                send_to_caps_oracle,
                                response_receiver,
                            ).await {
                                Err(e) => {
                                    send_to_loop
                                        .send(make_error_message(
                                            our_node.into(),
                                            id,
                                            source,
                                            e,
                                        ))
                                        .await
                                        .unwrap();
                                },
                                Ok(_) => {},
                            }
                            send_vfs_task_done.send(id).await.unwrap();
                        });
                    },
                }
            },
        }
    }
}

//  TODO: error handling: send error messages to caller
async fn handle_request(
    our_name: String,
    id: u64,
    source: Address,
    expects_response: Option<u64>,
    rsvp: Rsvp,
    request: VfsRequest,
    metadata: Option<String>,
    payload: Option<Payload>,
    new_caps: Vec<Capability>,
    vfs: Arc<Mutex<Vfs>>,
    send_to_loop: MessageSender,
    send_to_persist: tokio::sync::mpsc::Sender<bool>,
    send_to_terminal: PrintSender,
    send_to_caps_oracle: CapMessageSender,
    recv_response: MessageReceiver,
) -> Result<(), VfsError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    match &request {
        VfsRequest::New { identifier: _ } => {}
        VfsRequest::Add { identifier, .. }
        | VfsRequest::Rename { identifier, .. }
        | VfsRequest::Delete { identifier, .. }
        | VfsRequest::WriteOffset { identifier, .. } => {
            let _ = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_name.clone(),
                            process: ProcessId::Name("vfs".into()),
                        },
                        params: serde_json::to_string(&serde_json::json!({
                            "kind": "write",
                            "identifier": identifier,
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .unwrap();
            let has_cap = recv_cap_bool.await.unwrap();

            if !has_cap {
                return Err(VfsError::NoCap);
            }
        }
        VfsRequest::GetPath { identifier, .. }
        | VfsRequest::GetEntry { identifier, .. }
        | VfsRequest::GetFileChunk { identifier, .. }
        | VfsRequest::GetEntryLength { identifier, .. } => {
            let _ = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_name.clone(),
                            process: ProcessId::Name("vfs".into()),
                        },
                        params: serde_json::to_string(&serde_json::json!({
                            "kind": "read",
                            "identifier": identifier,
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .unwrap();
            let has_cap = recv_cap_bool.await.unwrap();

            if !has_cap {
                return Err(VfsError::NoCap);
            }
        }
    }

    let (ipc, bytes) = match_request(
        our_name.clone(),
        id.clone(),
        source.clone(),
        request,
        payload,
        new_caps,
        vfs,
        &send_to_loop,
        &send_to_persist,
        &send_to_terminal,
        recv_response,
    )
    .await?;

    //  TODO: properly handle rsvp
    if expects_response.is_some() {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_name.clone(),
                process: ProcessId::Name("vfs".into()),
            },
            target: Address {
                node: our_name.clone(),
                process: source.process.clone(),
            },
            rsvp,
            message: Message::Response((Response { ipc, metadata }, None)),
            payload: match bytes {
                Some(bytes) => Some(Payload {
                    mime: Some("application/octet-stream".into()),
                    bytes,
                }),
                None => None,
            },
            signed_capabilities: None,
        };

        let _ = send_to_loop.send(response).await;
    }

    Ok(())
}

#[async_recursion::async_recursion]
async fn match_request(
    our_name: String,
    id: u64,
    source: Address,
    request: VfsRequest,
    payload: Option<Payload>,
    new_caps: Vec<Capability>,
    vfs: Arc<Mutex<Vfs>>,
    send_to_loop: &MessageSender,
    send_to_persist: &tokio::sync::mpsc::Sender<bool>,
    send_to_terminal: &PrintSender,
    mut recv_response: MessageReceiver,
) -> Result<(Option<String>, Option<Vec<u8>>), VfsError> {
    Ok(match request {
        VfsRequest::New { identifier } => {
            for new_cap in new_caps {
                let _ = send_to_loop
                    .send(KernelMessage {
                        id,
                        source: Address {
                            node: our_name.clone(),
                            process: ProcessId::Name("vfs".into()),
                        },
                        target: Address {
                            node: our_name.clone(),
                            process: ProcessId::Name("kernel".into()),
                        },
                        rsvp: None,
                        message: Message::Request(Request {
                            inherit: false,
                            expects_response: None,
                            ipc: Some(
                                serde_json::to_string(&KernelCommand::GrantCapability {
                                    to_process: source.process.clone(),
                                    params: new_cap.params,
                                })
                                .unwrap(),
                            ),
                            metadata: None,
                        }),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await;
            }
            send_to_persist.send(true).await.unwrap();
            (
                Some(serde_json::to_string(&VfsResponse::New { identifier }).unwrap()),
                None,
            )
        }
        VfsRequest::Add {
            identifier,
            full_path,
            entry_type,
        } => {
            match entry_type {
                AddEntryType::Dir => {
                    if let Some(last_char) = full_path.chars().last() {
                        if last_char != '/' {
                            //  TODO: panic or correct & notify?
                            //  elsewhere we panic
                            // format!("{}/", full_path)
                            send_to_terminal
                                .send(Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "vfs: cannot add dir without trailing `/`: {}",
                                        full_path
                                    ),
                                })
                                .await
                                .unwrap();
                            panic!("");
                        };
                    } else {
                        panic!("empty path");
                    };
                    let mut vfs = vfs.lock().await;
                    if vfs.path_to_key.contains_key(&full_path) {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 0,
                                content: format!("vfs: not overwriting dir {}", full_path),
                            })
                            .await
                            .unwrap();
                        panic!(""); //  TODO: error?
                    };
                    let (name, parent_path) = make_dir_name(&full_path);
                    let Some(parent_key) = vfs.path_to_key.remove(&parent_path) else {
                        panic!("fp, pp: {}, {}", full_path, parent_path);
                    };
                    let key = Key::Dir { id: rand::random() };
                    vfs.key_to_entry.insert(
                        key.clone(),
                        Entry {
                            name,
                            full_path: full_path.clone(),
                            entry_type: EntryType::Dir {
                                parent: parent_key.clone(),
                                children: HashSet::new(),
                            },
                        },
                    );
                    vfs.path_to_key.insert(parent_path, parent_key);
                    vfs.path_to_key.insert(full_path.clone(), key.clone());
                }
                AddEntryType::NewFile => {
                    if let Some(last_char) = full_path.chars().last() {
                        if last_char == '/' {
                            send_to_terminal
                                .send(Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "vfs: file path cannot end with `/`: {}",
                                        full_path,
                                    ),
                                })
                                .await
                                .unwrap();
                            panic!("");
                        }
                    } else {
                        panic!("empty path");
                    };
                    let mut vfs = vfs.lock().await;
                    if vfs.path_to_key.contains_key(&full_path) {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 1,
                                content: format!("vfs: overwriting file {}", full_path),
                            })
                            .await
                            .unwrap();
                        let Some(old_key) = vfs.path_to_key.remove(&full_path) else {
                            panic!("");
                        };
                        vfs.key_to_entry.remove(&old_key);
                    };

                    let _ = send_to_loop
                        .send(KernelMessage {
                            id,
                            source: Address {
                                node: our_name.clone(),
                                process: ProcessId::Name("vfs".into()),
                            },
                            target: Address {
                                node: our_name.clone(),
                                process: ProcessId::Name("filesystem".into()),
                            },
                            rsvp: None,
                            message: Message::Request(Request {
                                inherit: true,
                                expects_response: Some(5), // TODO evaluate
                                ipc: Some(serde_json::to_string(&FsAction::Write).unwrap()),
                                metadata: None,
                            }),
                            payload,
                            signed_capabilities: None,
                        })
                        .await;
                    let write_response = recv_response.recv().await.unwrap();
                    let KernelMessage { message, .. } = write_response;
                    let Message::Response((Response { ipc, metadata: _ }, None)) = message else {
                        panic!("")
                    };
                    let Some(ipc) = ipc else {
                        panic!("");
                    };
                    let FsResponse::Write(hash) = serde_json::from_str(&ipc).unwrap() else {
                        panic!("");
                    };

                    let (name, parent_path) = make_file_name(&full_path);
                    let Some(parent_key) = vfs.path_to_key.remove(&parent_path) else {
                        panic!("");
                    };
                    let key = Key::File { id: hash };
                    vfs.key_to_entry.insert(
                        key.clone(),
                        Entry {
                            name,
                            full_path: full_path.clone(),
                            entry_type: EntryType::File {
                                parent: parent_key.clone(),
                            },
                        },
                    );
                    vfs.path_to_key.insert(parent_path, parent_key);
                    vfs.path_to_key.insert(full_path.clone(), key.clone());
                }
                AddEntryType::ExistingFile { hash } => {
                    if let Some(last_char) = full_path.chars().last() {
                        if last_char == '/' {
                            send_to_terminal
                                .send(Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "vfs: file path cannot end with `/`: {}",
                                        full_path,
                                    ),
                                })
                                .await
                                .unwrap();
                            panic!("");
                        }
                    } else {
                        panic!("empty path");
                    };
                    let mut vfs = vfs.lock().await;
                    if vfs.path_to_key.contains_key(&full_path) {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 1,
                                content: format!("vfs: overwriting file {}", full_path),
                            })
                            .await
                            .unwrap();
                        let Some(old_key) = vfs.path_to_key.remove(&full_path) else {
                            panic!("no old key");
                        };
                        vfs.key_to_entry.remove(&old_key);
                    };
                    let (name, parent_path) = make_file_name(&full_path);
                    let Some(parent_key) = vfs.path_to_key.remove(&parent_path) else {
                        panic!("");
                    };
                    let key = Key::File { id: hash };
                    vfs.key_to_entry.insert(
                        key.clone(),
                        Entry {
                            name,
                            full_path: full_path.clone(),
                            entry_type: EntryType::File {
                                parent: parent_key.clone(),
                            },
                        },
                    );
                    vfs.path_to_key.insert(parent_path, parent_key);
                    vfs.path_to_key.insert(full_path.clone(), key.clone());
                }
            }
            send_to_persist.send(true).await.unwrap();
            (
                Some(
                    serde_json::to_string(&VfsResponse::Add {
                        identifier,
                        full_path: full_path.clone(),
                    })
                    .unwrap(),
                ),
                None,
            )
        }
        VfsRequest::Rename {
            identifier,
            full_path,
            new_full_path,
        } => {
            let mut vfs = vfs.lock().await;
            let Some(key) = vfs.path_to_key.remove(&full_path) else {
                send_to_terminal
                    .send(Printout {
                        verbosity: 0,
                        content: format!("vfs: can't rename: nonexistent file {}", full_path),
                    })
                    .await
                    .unwrap();
                panic!("");
            };
            let Some(mut entry) = vfs.key_to_entry.remove(&key) else {
                send_to_terminal
                    .send(Printout {
                        verbosity: 0,
                        content: format!("vfs: can't rename: nonexistent file {}", full_path),
                    })
                    .await
                    .unwrap();
                panic!("");
            };
            match entry.entry_type {
                EntryType::Dir { .. } => {
                    if vfs.path_to_key.contains_key(&new_full_path) {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 0,
                                content: format!("vfs: not overwriting dir {}", new_full_path),
                            })
                            .await
                            .unwrap();
                        vfs.path_to_key.insert(full_path, key);
                        panic!(""); //  TODO: error?
                    };
                    let (name, _) = make_dir_name(&new_full_path);
                    entry.name = name;
                    entry.full_path = new_full_path.clone();
                    vfs.path_to_key.insert(new_full_path.clone(), key.clone());
                    vfs.key_to_entry.insert(key, entry);
                    //  TODO: recursively apply path update to all children
                    //  update_child_paths(full_path, new_full_path, children);
                }
                EntryType::File { parent: _ } => {
                    if vfs.path_to_key.contains_key(&new_full_path) {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 1,
                                content: format!("vfs: overwriting file {}", new_full_path),
                            })
                            .await
                            .unwrap();
                    };
                    let (name, _) = make_file_name(&new_full_path);
                    entry.name = name;
                    entry.full_path = new_full_path.clone();
                    vfs.path_to_key.insert(new_full_path.clone(), key.clone());
                    vfs.key_to_entry.insert(key, entry);
                }
            }
            send_to_persist.send(true).await.unwrap();
            (
                Some(
                    serde_json::to_string(&VfsResponse::Rename {
                        identifier,
                        new_full_path,
                    })
                    .unwrap(),
                ),
                None,
            )
        }
        VfsRequest::Delete {
            identifier,
            full_path,
        } => {
            let mut vfs = vfs.lock().await;
            let Some(key) = vfs.path_to_key.remove(&full_path) else {
                send_to_terminal
                    .send(Printout {
                        verbosity: 0,
                        content: format!("vfs: can't delete: nonexistent entry {}", full_path),
                    })
                    .await
                    .unwrap();
                panic!("");
            };
            let Some(entry) = vfs.key_to_entry.remove(&key) else {
                send_to_terminal
                    .send(Printout {
                        verbosity: 0,
                        content: format!("vfs: can't delete: nonexistent entry {}", full_path),
                    })
                    .await
                    .unwrap();
                panic!("");
            };
            match entry.entry_type {
                EntryType::Dir {
                    parent: _,
                    ref children,
                } => {
                    if !children.is_empty() {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 0,
                                content: format!(
                                    "vfs: can't delete: non-empty directory {}",
                                    full_path
                                ),
                            })
                            .await
                            .unwrap();
                        vfs.path_to_key.insert(full_path.clone(), key.clone());
                        vfs.key_to_entry.insert(key.clone(), entry);
                    }
                }
                EntryType::File { parent } => {
                    match vfs.key_to_entry.get_mut(&parent) {
                        None => {
                            send_to_terminal
                                .send(Printout {
                                    verbosity: 0,
                                    content: format!(
                                        "vfs: delete: unexpected file with no parent dir: {}",
                                        full_path
                                    ),
                                })
                                .await
                                .unwrap();
                            panic!("");
                        }
                        Some(parent) => {
                            let EntryType::Dir {
                                parent: _,
                                ref mut children,
                            } = parent.entry_type
                            else {
                                panic!("");
                            };
                            //  TODO: does this work?
                            children.remove(&key);
                        }
                    }
                }
            }
            send_to_persist.send(true).await.unwrap();
            (
                Some(
                    serde_json::to_string(&VfsResponse::Delete {
                        identifier,
                        full_path,
                    })
                    .unwrap(),
                ),
                None,
            )
        }
        VfsRequest::WriteOffset {
            identifier,
            full_path,
            offset,
        } => {
            let file_hash = {
                let mut vfs = vfs.lock().await;
                let Some(key) = vfs.path_to_key.remove(&full_path) else {
                    panic!("");
                };
                let key2 = key.clone();
                let Key::File { id: file_hash } = key2 else {
                    panic!(""); //  TODO
                };
                vfs.path_to_key.insert(full_path.clone(), key);
                file_hash
            };
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our_name.clone(),
                        process: ProcessId::Name("vfs".into()),
                    },
                    target: Address {
                        node: our_name.clone(),
                        process: ProcessId::Name("filesystem".into()),
                    },
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: true,
                        expects_response: Some(5), // TODO evaluate
                        ipc: Some(
                            serde_json::to_string(&FsAction::WriteOffset((file_hash, offset)))
                                .unwrap(),
                        ),
                        metadata: None,
                    }),
                    payload,
                    signed_capabilities: None,
                })
                .await;

            (
                Some(
                    serde_json::to_string(&VfsResponse::WriteOffset {
                        identifier,
                        full_path,
                        offset,
                    })
                    .unwrap(),
                ),
                None,
            )
        }
        VfsRequest::GetPath { identifier, hash } => {
            let mut vfs = vfs.lock().await;
            let key = Key::File { id: hash.clone() };
            let ipc = Some(
                serde_json::to_string(&VfsResponse::GetPath {
                    identifier,
                    hash,
                    full_path: match vfs.key_to_entry.remove(&key) {
                        None => None,
                        Some(entry) => {
                            let full_path = entry.full_path.clone();
                            vfs.key_to_entry.insert(key, entry);
                            Some(full_path)
                        }
                    },
                })
                .unwrap(),
            );
            (ipc, None)
        }
        VfsRequest::GetEntry {
            identifier,
            ref full_path,
        } => {
            let (key, entry, paths) = {
                let mut vfs = vfs.lock().await;
                let key = vfs.path_to_key.remove(full_path);
                match key {
                    None => (None, None, vec![]),
                    Some(key) => {
                        vfs.path_to_key.insert(full_path.clone(), key.clone());
                        let entry = vfs.key_to_entry.remove(&key);
                        match entry {
                            None => (Some(key), None, vec![]),
                            Some(ref e) => {
                                vfs.key_to_entry.insert(key.clone(), e.clone());
                                match e.entry_type {
                                    EntryType::File { parent: _ } => (Some(key), entry, vec![]),
                                    EntryType::Dir {
                                        parent: _,
                                        ref children,
                                    } => {
                                        let mut paths: Vec<String> = Vec::new();
                                        for child in children {
                                            let Some(child) = vfs.key_to_entry.get(&child) else {
                                                send_to_terminal
                                                    .send(Printout {
                                                        verbosity: 0,
                                                        content: format!(
                                                            "vfs: child missing for: {}",
                                                            full_path
                                                        ),
                                                    })
                                                    .await
                                                    .unwrap();
                                                continue;
                                            };
                                            paths.push(child.full_path.clone());
                                        }
                                        paths.sort();
                                        (Some(key), entry, paths)
                                    }
                                }
                            }
                        }
                    }
                }
            };

            let entry_not_found = (
                Some(
                    serde_json::to_string(&VfsResponse::GetEntry {
                        identifier: identifier.clone(),
                        full_path: full_path.clone(),
                        children: vec![],
                    })
                    .unwrap(),
                ),
                None,
            );
            match key {
                None => entry_not_found,
                Some(key) => match entry {
                    None => entry_not_found,
                    Some(entry) => match entry.entry_type {
                        EntryType::Dir {
                            parent: _,
                            children: _,
                        } => (
                            Some(
                                serde_json::to_string(&VfsResponse::GetEntry {
                                    identifier,
                                    full_path: full_path.clone(),
                                    children: paths,
                                })
                                .unwrap(),
                            ),
                            None,
                        ),
                        EntryType::File { parent: _ } => {
                            let Key::File { id: file_hash } = key else {
                                panic!("");
                            };
                            let _ = send_to_loop
                                .send(KernelMessage {
                                    id,
                                    source: Address {
                                        node: our_name.clone(),
                                        process: ProcessId::Name("vfs".into()),
                                    },
                                    target: Address {
                                        node: our_name.clone(),
                                        process: ProcessId::Name("filesystem".into()),
                                    },
                                    rsvp: None,
                                    message: Message::Request(Request {
                                        inherit: true,
                                        expects_response: Some(5), // TODO evaluate
                                        ipc: Some(
                                            serde_json::to_string(&FsAction::Read(
                                                file_hash.clone(),
                                            ))
                                            .unwrap(),
                                        ),
                                        metadata: None,
                                    }),
                                    payload: None,
                                    signed_capabilities: None,
                                })
                                .await;
                            let read_response = recv_response.recv().await.unwrap();
                            let KernelMessage {
                                message, payload, ..
                            } = read_response;
                            let Message::Response((Response { ipc, metadata: _ }, None)) = message
                            else {
                                panic!("")
                            };
                            let Some(ipc) = ipc else {
                                panic!("");
                            };
                            let Ok(FsResponse::Read(read_hash)) =
                                serde_json::from_str::<Result<FsResponse, FileSystemError>>(&ipc)
                                    .unwrap()
                            else {
                                panic!("");
                            };
                            assert_eq!(file_hash, read_hash);
                            let Some(payload) = payload else {
                                panic!("");
                            };
                            (
                                Some(
                                    serde_json::to_string(&VfsResponse::GetEntry {
                                        identifier,
                                        full_path: full_path.clone(),
                                        children: vec![],
                                    })
                                    .unwrap(),
                                ),
                                Some(payload.bytes),
                            )
                        }
                    },
                },
            }
        }
        VfsRequest::GetFileChunk {
            identifier,
            full_path,
            offset,
            length,
        } => {
            let file_hash = {
                let mut vfs = vfs.lock().await;
                let Some(key) = vfs.path_to_key.remove(&full_path) else {
                    panic!(""); //  TODO
                };
                let key2 = key.clone();
                let Key::File { id: file_hash } = key2 else {
                    panic!(""); //  TODO
                };
                vfs.path_to_key.insert(full_path.clone(), key);
                file_hash
            };

            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our_name.clone(),
                        process: ProcessId::Name("vfs".into()),
                    },
                    target: Address {
                        node: our_name.clone(),
                        process: ProcessId::Name("filesystem".into()),
                    },
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: true,
                        expects_response: Some(5), // TODO evaluate
                        ipc: Some(
                            serde_json::to_string(&FsAction::ReadChunk(ReadChunkRequest {
                                file: file_hash.clone(),
                                start: offset,
                                length,
                            }))
                            .unwrap(),
                        ),
                        metadata: None,
                    }),
                    payload: None,
                    signed_capabilities: None,
                })
                .await;
            let read_response = recv_response.recv().await.unwrap();
            let KernelMessage {
                message, payload, ..
            } = read_response;
            let Message::Response((Response { ipc, metadata: _ }, None)) = message else {
                panic!("")
            };
            let Some(ipc) = ipc else {
                panic!("");
            };
            let Ok(FsResponse::ReadChunk(read_hash)) =
                serde_json::from_str::<Result<FsResponse, FileSystemError>>(&ipc).unwrap()
            else {
                panic!("");
            };
            assert_eq!(file_hash, read_hash);
            let Some(payload) = payload else {
                panic!("");
            };

            (
                Some(
                    serde_json::to_string(&VfsResponse::GetFileChunk {
                        identifier,
                        full_path,
                        offset,
                        length,
                    })
                    .unwrap(),
                ),
                Some(payload.bytes),
            )
        }
        VfsRequest::GetEntryLength {
            identifier,
            full_path,
        } => {
            if full_path.chars().last() == Some('/') {
                (
                    Some(
                        serde_json::to_string(&VfsResponse::GetEntryLength {
                            identifier,
                            full_path,
                            length: 0,
                        })
                        .unwrap(),
                    ),
                    None,
                )
            } else {
                let file_hash = {
                    let mut vfs = vfs.lock().await;
                    let Some(key) = vfs.path_to_key.remove(&full_path) else {
                        panic!("");
                    };
                    let key2 = key.clone();
                    let Key::File { id: file_hash } = key2 else {
                        panic!(""); //  TODO
                    };
                    vfs.path_to_key.insert(full_path.clone(), key);
                    file_hash
                };

                let _ = send_to_loop
                    .send(KernelMessage {
                        id,
                        source: Address {
                            node: our_name.clone(),
                            process: ProcessId::Name("vfs".into()),
                        },
                        target: Address {
                            node: our_name.clone(),
                            process: ProcessId::Name("filesystem".into()),
                        },
                        rsvp: None,
                        message: Message::Request(Request {
                            inherit: true,
                            expects_response: Some(5), // TODO evaluate
                            ipc: Some(serde_json::to_string(&FsAction::Length(file_hash)).unwrap()),
                            metadata: None,
                        }),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await;
                let length_response = recv_response.recv().await.unwrap();
                let KernelMessage { message, .. } = length_response;
                let Message::Response((Response { ipc, metadata: _ }, None)) = message else {
                    panic!("")
                };
                let Some(ipc) = ipc else {
                    panic!("");
                };
                let Ok(FsResponse::Length(length)) =
                    serde_json::from_str::<Result<FsResponse, FileSystemError>>(&ipc).unwrap()
                else {
                    panic!("");
                };

                (
                    Some(
                        serde_json::to_string(&VfsResponse::GetEntryLength {
                            identifier,
                            full_path,
                            length,
                        })
                        .unwrap(),
                    ),
                    None,
                )
            }
        }
    })
}
