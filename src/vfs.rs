use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::prelude::*;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

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
type DriveToVfs = HashMap<String, Arc<Mutex<Vfs>>>;
type DriveToVfsSerializable = HashMap<String, Vfs>;

type RequestQueue = VecDeque<(KernelMessage, MessageReceiver)>;
type DriveToQueue = Arc<Mutex<HashMap<String, RequestQueue>>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Entry {
    name: String,
    full_path: String,
    entry_type: EntryType,
    // ...  //  general metadata?
}

#[derive(Clone, Debug, Deserialize, Serialize)]
enum EntryType {
    Dir { parent: Key, children: HashSet<Key> },
    File { parent: Key }, //  hash could be generalized to `location` if we want to be able to point at, e.g., remote files
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

fn clean_path(path: &str) -> String {
    let cleaned = path.trim_start_matches('/').trim_end_matches('/');
    format!("/{}", cleaned)
}

fn get_parent_path(path: &str) -> String {
    let mut split_path: Vec<&str> = path.split("/").collect();
    split_path.pop();
    let parent_path = split_path.join("/");
    if parent_path.is_empty() {
        "/".to_string()
    } else {
        parent_path
    }
}

#[async_recursion::async_recursion]
async fn create_entry(vfs: &mut MutexGuard<Vfs>, path: &str, key: Key) -> Result<Key, VfsError> {
    if let Some(existing_key) = vfs.path_to_key.get(path) {
        return Ok(existing_key.clone());
    }

    let parent_path = get_parent_path(path);
    let parent_key = create_entry(vfs, &parent_path, Key::Dir { id: rand::random() }).await?;

    let entry_type = match key {
        Key::Dir { id } => EntryType::Dir {
            parent: parent_key.clone(),
            children: HashSet::new(),
        },
        Key::File { id } => EntryType::File {
            parent: parent_key.clone(),
        },
    };
    let entry = Entry {
        name: path.split("/").last().unwrap().to_string(),
        full_path: path.to_string(),
        entry_type: entry_type,
    };
    vfs.key_to_entry.insert(key.clone(), entry);
    vfs.path_to_key.insert(path.to_string(), key.clone());

    if let Some(parent_entry) = vfs.key_to_entry.get_mut(&parent_key) {
        if let EntryType::Dir { children, .. } = &mut parent_entry.entry_type {
            children.insert(key.clone());
        }
    }

    Ok(key)
}

#[async_recursion::async_recursion]
async fn rename_entry(
    vfs: Arc<Mutex<Vfs>>,
    old_path: &str,
    new_path: &str,
) -> Result<(), VfsError> {
    let (key, children) = {
        let mut vfs = vfs.lock().await;
        let key = match vfs.path_to_key.remove(old_path) {
            Some(key) => key,
            None => return Err(VfsError::EntryNotFound),
        };
        let mut entry = match vfs.key_to_entry.get_mut(&key) {
            Some(entry) => entry,
            None => return Err(VfsError::EntryNotFound),
        };
        entry.name = new_path.split("/").last().unwrap().to_string();
        entry.full_path = new_path.to_string();
        let children = if let EntryType::Dir { children, .. } = &entry.entry_type {
            children.clone()
        } else {
            HashSet::new()
        };
        vfs.path_to_key.insert(new_path.to_string(), key.clone());
        (key, children)
    };

    // recursively update the paths of the children
    for child_key in children {
        let child_entry = {
            let vfs = vfs.lock().await;
            vfs.key_to_entry.get(&child_key).unwrap().clone()
        };
        let old_child_path = child_entry.full_path.clone();
        let new_child_path = old_child_path.replace(old_path, new_path);
        rename_entry(vfs.clone(), &old_child_path, &new_child_path).await?;
    }

    Ok(())
}

fn make_error_message(
    our_node: String,
    id: u64,
    source: Address,
    error: VfsError,
) -> KernelMessage {
    KernelMessage {
        id,
        source: Address {
            node: our_node,
            process: VFS_PROCESS_ID.clone(),
        },
        target: source,
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec(&VfsResponse::Err(error)).unwrap(), //  TODO: handle error?
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}

async fn state_to_bytes(state: &DriveToVfs) -> Vec<u8> {
    let mut serializable: DriveToVfsSerializable = HashMap::new();
    for (id, vfs) in state.iter() {
        let vfs = vfs.lock().await;
        serializable.insert(id.clone(), (*vfs).clone());
    }
    bincode::serialize(&serializable).unwrap()
}

fn bytes_to_state(bytes: &Vec<u8>, state: &mut DriveToVfs) {
    let serializable: DriveToVfsSerializable = bincode::deserialize(&bytes).unwrap();
    for (id, vfs) in serializable.into_iter() {
        state.insert(id, Arc::new(Mutex::new(vfs)));
    }
}

async fn send_persist_state_message(
    our_node: String,
    send_to_loop: MessageSender,
    id: u64,
    state: Vec<u8>,
) {
    let _ = send_to_loop
        .send(KernelMessage {
            id,
            source: Address {
                node: our_node.clone(),
                process: VFS_PROCESS_ID.clone(),
            },
            target: Address {
                node: our_node,
                process: FILESYSTEM_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: true,
                expects_response: Some(5), // TODO evaluate
                ipc: serde_json::to_vec(&FsAction::SetState(VFS_PROCESS_ID.clone())).unwrap(),
                metadata: None,
            }),
            payload: Some(Payload {
                mime: None,
                bytes: state,
            }),
            signed_capabilities: None,
        })
        .await;
}

async fn persist_state(
    send_to_persist: &tokio::sync::mpsc::Sender<u64>,
    recv_response: &mut MessageReceiver,
    id: u64,
) -> Result<(), VfsError> {
    send_to_persist
        .send(id)
        .await
        .map_err(|_| VfsError::PersistError)?;
    let persist_response = recv_response.recv().await.ok_or(VfsError::PersistError)?;
    let KernelMessage { message, .. } = persist_response;
    let Message::Response((Response { ipc, .. }, None)) = message else {
        return Err(VfsError::PersistError);
    };
    let ipc = ipc.ok_or(VfsError::PersistError)?;
    let response = serde_json::from_str::<Result<FsResponse, FsError>>(&ipc)
        .map_err(|_| VfsError::PersistError)?;
    match response {
        Ok(FsResponse::SetState) => Ok(()),
        _ => Err(VfsError::PersistError),
    }
}

async fn load_state_from_reboot(
    our_node: String,
    send_to_loop: &MessageSender,
    recv_from_loop: &mut MessageReceiver,
    drive_to_vfs: &mut DriveToVfs,
) {
    let _ = send_to_loop
        .send(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our_node.clone(),
                process: VFS_PROCESS_ID.clone(),
            },
            target: Address {
                node: our_node.clone(),
                process: FILESYSTEM_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: true,
                expects_response: Some(5), // TODO evaluate
                ipc: serde_json::to_vec(&FsAction::GetState(VFS_PROCESS_ID.clone())).unwrap(),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        })
        .await;
    let km = recv_from_loop.recv().await;
    let Some(km) = km else {
        return ();
    };

    let KernelMessage {
        message, payload, ..
    } = km;
    let Message::Response((Response { ipc, .. }, None)) = message else {
        return ();
    };
    let Ok(Ok(FsResponse::GetState)) = serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc)
    else {
        return ();
    };
    let Some(payload) = payload else {
        return ();
    };
    bytes_to_state(&payload.bytes, drive_to_vfs);
}

pub async fn vfs(
    our_node: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    vfs_messages: Vec<KernelMessage>,
) -> anyhow::Result<()> {
    let mut drive_to_vfs: DriveToVfs = HashMap::new();
    let drive_to_queue: DriveToQueue = Arc::new(Mutex::new(HashMap::new()));
    let mut response_router: ResponseRouter = HashMap::new();
    let (send_vfs_task_done, mut recv_vfs_task_done): (
        tokio::sync::mpsc::Sender<u64>,
        tokio::sync::mpsc::Receiver<u64>,
    ) = tokio::sync::mpsc::channel(VFS_TASK_DONE_CHANNEL_CAPACITY);
    let (send_persist_state, mut recv_persist_state): (
        tokio::sync::mpsc::Sender<u64>,
        tokio::sync::mpsc::Receiver<u64>,
    ) = tokio::sync::mpsc::channel(VFS_PERSIST_STATE_CHANNEL_CAPACITY);

    load_state_from_reboot(
        our_node.clone(),
        &send_to_loop,
        &mut recv_from_loop,
        &mut drive_to_vfs,
    )
    .await;

    for vfs_message in vfs_messages {
        send_to_loop.send(vfs_message).await.unwrap();
    }

    loop {
        tokio::select! {
            id_done = recv_vfs_task_done.recv() => {
                let Some(id_done) = id_done else { continue };
                response_router.remove(&id_done);
            },
            respond_to_id = recv_persist_state.recv() => {
                let Some(respond_to_id) = respond_to_id else { continue };
                let our_node = our_node.clone();
                let send_to_loop = send_to_loop.clone();
                let serialized_state = state_to_bytes(&drive_to_vfs).await;
                send_persist_state_message(
                    our_node.clone(),
                    send_to_loop,
                    respond_to_id,
                    serialized_state,
                ).await;
            },
            km = recv_from_loop.recv() => {
                let Some(km) = km else {
                    continue;
                };
                if let Some(response_sender) = response_router.get(&km.id) {
                    let _ = response_sender.send(km).await;
                    continue;
                }

                let KernelMessage {
                    id,
                    source,
                    message,
                    ..
                } = km.clone();
                let Message::Request(Request {
                    ipc,
                    ..
                }) = message.clone()
                else {
                    // consider moving this handling into it's own function
                    continue;
                };

                let request: VfsRequest = match serde_json::from_slice(&ipc) {
                    Ok(r) => r,
                    Err(e) => {
                        println!("vfs: got invalid Request: {}", e);
                        continue;
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

                let (response_sender, response_receiver): (
                    MessageSender,
                    MessageReceiver,
                ) = tokio::sync::mpsc::channel(VFS_RESPONSE_CHANNEL_CAPACITY);
                response_router.insert(km.id.clone(), response_sender);

                let mut drive_to_queue_lock = drive_to_queue.lock().await;
                match drive_to_queue_lock.remove(&request.drive) {
                    Some(mut queue) => {
                        queue.push_back((km, response_receiver));
                        drive_to_queue_lock.insert(request.drive, queue);
                    },
                    None => {
                        let mut queue: RequestQueue = VecDeque::new();
                        queue.push_back((km, response_receiver));
                        drive_to_queue_lock.insert(request.drive.clone(), queue);

                        let (vfs, new_caps) = match drive_to_vfs.get(&request.drive) {
                            Some(vfs) => (Arc::clone(vfs), vec![]),
                            None => {
                                let VfsAction::New = request.action else {
                                    // clean up queue
                                    match drive_to_queue_lock.remove(&request.drive) {
                                        None => {},
                                        Some(mut queue) => {
                                            let _ = queue.pop_back();
                                            if !queue.is_empty() {
                                                drive_to_queue_lock.insert(request.drive, queue);
                                            }
                                        },
                                    }
                                    send_to_loop
                                        .send(make_error_message(
                                            our_node.clone(),
                                            id,
                                            source.clone(),
                                            VfsError::BadDriveName,
                                        ))
                                        .await
                                        .unwrap();
                                    continue;
                                };
                                drive_to_vfs.insert(
                                    request.drive.clone(),
                                    Arc::new(Mutex::new(Vfs::new())),
                                );
                                let read_cap = Capability {
                                    issuer: Address {
                                        node: our_node.clone(),
                                        process: VFS_PROCESS_ID.clone(),
                                    },
                                    params: serde_json::to_string(
                                        &serde_json::json!({"kind": "read", "drive": request.drive})
                                    ).unwrap(),
                                };
                                let write_cap = Capability {
                                    issuer: Address {
                                        node: our_node.clone(),
                                        process: VFS_PROCESS_ID.clone(),
                                    },
                                    params: serde_json::to_string(
                                        &serde_json::json!({"kind": "write", "drive": request.drive})
                                    ).unwrap(),
                                };

                                (
                                    Arc::clone(drive_to_vfs.get(&request.drive).unwrap()),
                                    vec![read_cap, write_cap],
                                )
                            }
                        };

                        let our_node = our_node.clone();
                        let drive = request.drive.clone();
                        let send_to_loop = send_to_loop.clone();
                        let send_persist_state = send_persist_state.clone();
                        let send_to_terminal = send_to_terminal.clone();
                        let send_to_caps_oracle = send_to_caps_oracle.clone();
                        let send_vfs_task_done = send_vfs_task_done.clone();
                        let drive_to_queue = Arc::clone(&drive_to_queue);
                        match &message {
                            Message::Response(_) => {},
                            Message::Request(_) => {
                                tokio::spawn(async move {
                                    loop {
                                        let our_node = our_node.clone();
                                        let drive = drive.clone();
                                        let next_message = {
                                            let mut drive_to_queue_lock = drive_to_queue.lock().await;
                                            match drive_to_queue_lock.remove(&drive) {
                                                None => None,
                                                Some(mut queue) => {
                                                    let next_message = queue.pop_front();
                                                    drive_to_queue_lock.insert(drive.clone(), queue);
                                                    next_message
                                                },
                                            }
                                        };
                                        match next_message {
                                            None => {
                                                // queue is empty
                                                let mut drive_to_queue_lock = drive_to_queue.lock().await;
                                                match drive_to_queue_lock.remove(&drive) {
                                                    None => {},
                                                    Some(queue) => {
                                                        if queue.is_empty() {}
                                                        else {
                                                            // between setting next_message
                                                            //  and lock() in this block, new
                                                            //  entry was added to queue:
                                                            //  process it
                                                            drive_to_queue_lock.insert(drive, queue);
                                                            continue;
                                                        }
                                                    },
                                                }
                                                let _ = send_vfs_task_done.send(id).await;
                                                return ();
                                            },
                                            Some((km, response_receiver)) => {
                                                // handle next item
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
                                                    ipc,
                                                    metadata, // we return this to Requester for kernel reasons
                                                    ..
                                                }) = message.clone()
                                                else {
                                                    continue;
                                                };

                                                let request: VfsRequest = match serde_json::from_slice(&ipc) {
                                                    Ok(r) => r,
                                                    Err(e) => {
                                                        println!("vfs: got invalid Request: {}", e);
                                                        continue;
                                                    }
                                                };
                                                match handle_request(
                                                    our_node.clone(),
                                                    id,
                                                    source.clone(),
                                                    expects_response,
                                                    rsvp,
                                                    request,
                                                    metadata,
                                                    payload,
                                                    new_caps.clone(),
                                                    Arc::clone(&vfs),
                                                    send_to_loop.clone(),
                                                    send_persist_state.clone(),
                                                    send_to_terminal.clone(),
                                                    send_to_caps_oracle.clone(),
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
                                            },
                                        }
                                    }
                                });
                            },
                        }
                    },
                }
            },
        }
    }
}

async fn handle_request(
    our_node: String,
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
    send_to_persist: tokio::sync::mpsc::Sender<u64>,
    send_to_terminal: PrintSender,
    send_to_caps_oracle: CapMessageSender,
    recv_response: MessageReceiver,
) -> Result<(), VfsError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    match &request.action {
        VfsAction::Add { .. }
        | VfsAction::Rename { .. }
        | VfsAction::Delete { .. }
        | VfsAction::WriteOffset { .. }
        | VfsAction::SetSize { .. } => {
            let _ = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: VFS_PROCESS_ID.clone(),
                        },
                        params: serde_json::to_string(&serde_json::json!({
                            "kind": "write",
                            "drive": request.drive,
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .await
                .unwrap();
            let has_cap = recv_cap_bool.await.unwrap();
            if !has_cap {
                return Err(VfsError::NoCap);
            }
        }
        VfsAction::GetPath { .. }
        | VfsAction::GetHash { .. }
        | VfsAction::GetEntry { .. }
        | VfsAction::GetFileChunk { .. }
        | VfsAction::GetEntryLength { .. } => {
            let _ = send_to_caps_oracle
                .send(CapMessage::Has {
                    on: source.process.clone(),
                    cap: Capability {
                        issuer: Address {
                            node: our_node.clone(),
                            process: VFS_PROCESS_ID.clone(),
                        },
                        params: serde_json::to_string(&serde_json::json!({
                            "kind": "read",
                            "drive": request.drive,
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .await
                .unwrap();
            let has_cap = recv_cap_bool.await.unwrap();
            if !has_cap {
                return Err(VfsError::NoCap);
            }
        }
        _ => {} // New
    }

    let (ipc, bytes) = match_request(
        our_node.clone(),
        id.clone(),
        source.clone(),
        request,
        payload,
        new_caps,
        vfs,
        &send_to_loop,
        &send_to_persist,
        &send_to_terminal,
        &send_to_caps_oracle,
        recv_response,
    )
    .await?;

    //  TODO: properly handle rsvp
    if expects_response.is_some() {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_node.clone(),
                process: VFS_PROCESS_ID.clone(),
            },
            target: Address {
                node: our_node.clone(),
                process: source.process.clone(),
            },
            rsvp,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc,
                    metadata,
                },
                None,
            )),
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

// #[async_recursion::async_recursion]
async fn match_request(
    our_node: String,
    id: u64,
    source: Address,
    request: VfsRequest,
    payload: Option<Payload>,
    new_caps: Vec<Capability>,
    vfs: Arc<Mutex<Vfs>>,
    send_to_loop: &MessageSender,
    send_to_persist: &tokio::sync::mpsc::Sender<u64>,
    send_to_terminal: &PrintSender,
    send_to_caps_oracle: &CapMessageSender,
    mut recv_response: MessageReceiver,
) -> Result<(Vec<u8>, Option<Vec<u8>>), VfsError> {
    Ok(match request.action {
        VfsAction::New => {
            for new_cap in new_caps {
                let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
                let _ = send_to_caps_oracle
                    .send(CapMessage::Add {
                        on: source.process.clone(),
                        cap: new_cap,
                        responder: send_cap_bool,
                    })
                    .await
                    .unwrap();
                let _ = recv_cap_bool.await.unwrap();
            }
            match persist_state(send_to_persist, &mut recv_response, id).await {
                Err(_) => return Err(VfsError::PersistError),
                Ok(_) => return Ok((Some(serde_json::to_string(&VfsResponse::Ok).unwrap()), None)),
            }
        }
        VfsAction::Add {
            mut full_path,
            entry_type,
        } => {
            match entry_type {
                AddEntryType::Dir => {
                    full_path = clean_path(&full_path);

                    let mut vfs = vfs.lock().await;
                    if vfs.path_to_key.contains_key(&full_path) {
                        send_to_terminal
                            .send(Printout {
                                verbosity: 0,
                                content: format!("vfs: not overwriting dir {}", full_path),
                            })
                            .await
                            .unwrap();
                        return Ok((Some(serde_json::to_string(&VfsResponse::Ok).unwrap()), None));
                    };
                    match create_entry(&mut vfs, &full_path, Key::Dir { id: rand::random() }).await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                AddEntryType::NewFile => {
                    full_path = clean_path(&full_path);
                    let hash = {
                        let mut vfs = vfs.lock().await;
                        if !vfs.path_to_key.contains_key(&full_path) {
                            None
                        } else {
                            send_to_terminal
                                .send(Printout {
                                    verbosity: 1,
                                    content: format!("vfs: overwriting file {}", full_path),
                                })
                                .await
                                .unwrap();
                            match vfs.path_to_key.remove(&full_path) {
                                None => None,
                                Some(key) => {
                                    let Key::File { id: hash } = key else {
                                        return Err(VfsError::InternalError);
                                    };
                                    Some(hash)
                                }
                            }
                            // vfs.key_to_entry.remove(&old_key);
                        }
                    };

                    let _ = send_to_loop
                        .send(KernelMessage {
                            id,
                            source: Address {
                                node: our_node.clone(),
                                process: VFS_PROCESS_ID.clone(),
                            },
                            target: Address {
                                node: our_node.clone(),
                                process: FILESYSTEM_PROCESS_ID.clone(),
                            },
                            rsvp: None,
                            message: Message::Request(Request {
                                inherit: true,
                                expects_response: Some(5), // TODO evaluate
                                ipc: serde_json::to_vec(&FsAction::Write(hash)).unwrap(),
                                metadata: None,
                            }),
                            payload,
                            signed_capabilities: None,
                        })
                        .await;
                    let write_response = recv_response.recv().await.unwrap();
                    let KernelMessage { message, .. } = write_response;
                    let Message::Response((Response { ipc, .. }, None)) = message else {
                        return Err(VfsError::InternalError);
                    };

                    let Some(ipc) = ipc else {
                        return Err(VfsError::InternalError);
                    };

                    let Ok(FsResponse::Write(hash)) =
                        serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc).unwrap()
                    else {
                        return Err(VfsError::InternalError);
                    };

                    match create_entry(&mut vfs.lock().await, &full_path, Key::File { id: hash })
                        .await
                    {
                        Ok(_) => {}
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                AddEntryType::ExistingFile { hash } => {
                    full_path = clean_path(&full_path);

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
                            println!("no old key");
                            return Err(VfsError::InternalError);
                        };
                        vfs.key_to_entry.remove(&old_key);
                    };
                    match create_entry(&mut vfs, &full_path, Key::File { id: hash }).await {
                        Ok(_) => {}
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                AddEntryType::ZipArchive => {
                    let Some(payload) = payload else {
                        return Err(VfsError::InternalError);
                    };
                    let Some(mime) = payload.mime else {
                        return Err(VfsError::InternalError);
                    };
                    if "application/zip" != mime {
                        return Err(VfsError::InternalError);
                    }
                    let file = std::io::Cursor::new(&payload.bytes);
                    let mut zip = match zip::ZipArchive::new(file) {
                        Ok(f) => f,
                        Err(e) => panic!("vfs: zip error: {:?}", e),
                    };

                    // loop through items in archive; recursively add to root
                    for i in 0..zip.len() {
                        // must destruct the zip file created in zip.by_index()
                        //  Before any `.await`s are called since ZipFile is not
                        //  Send and so does not play nicely with await
                        let (is_file, is_dir, full_path, file_contents) = {
                            let mut file = zip.by_index(i).unwrap();
                            let is_file = file.is_file();
                            let is_dir = file.is_dir();
                            let full_path = format!("/{}", file.name());
                            let mut file_contents = Vec::new();
                            if is_file {
                                file.read_to_end(&mut file_contents).unwrap();
                            };
                            (is_file, is_dir, full_path, file_contents)
                        };
                        if is_file {
                            let hash = {
                                let vfs = vfs.lock().await;
                                if !vfs.path_to_key.contains_key(&full_path) {
                                    None
                                } else {
                                    send_to_terminal
                                        .send(Printout {
                                            verbosity: 1,
                                            content: format!("vfs: overwriting file {}", full_path),
                                        })
                                        .await
                                        .unwrap();
                                    match vfs.path_to_key.get(&full_path) {
                                        None => None,
                                        Some(key) => {
                                            let Key::File { id: hash } = key else {
                                                return Err(VfsError::InternalError);
                                            };
                                            Some(*hash)
                                        }
                                    }
                                    // vfs.key_to_entry.remove(&old_key);
                                }
                            };
                            let _ = send_to_loop
                                .send(KernelMessage {
                                    id,
                                    source: Address {
                                        node: our_node.clone(),
                                        process: VFS_PROCESS_ID.clone(),
                                    },
                                    target: Address {
                                        node: our_node.clone(),
                                        process: FILESYSTEM_PROCESS_ID.clone(),
                                    },
                                    rsvp: None,
                                    message: Message::Request(Request {
                                        inherit: true,
                                        expects_response: Some(5), // TODO evaluate
                                        ipc: serde_json::to_vec(&FsAction::Write(hash)).unwrap(),
                                        metadata: None,
                                    }),
                                    payload: Some(Payload {
                                        mime: None,
                                        bytes: file_contents,
                                    }),
                                    signed_capabilities: None,
                                })
                                .await;
                            let write_response = match recv_response.recv().await {
                                Some(response) => response,
                                None => {
                                    println!("No response received...");
                                    continue;
                                }
                            };
                            let KernelMessage { message, .. } = write_response;
                            let Message::Response((Response { ipc, .. }, None)) = message else {
                                return Err(VfsError::InternalError);
                            };
                            let Some(ipc) = ipc else {
                                return Err(VfsError::InternalError);
                            };

                            let Ok(FsResponse::Write(hash)) =
                                serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc)
                                    .unwrap()
                            else {
                                return Err(VfsError::InternalError);
                            };

                            match create_entry(
                                &mut vfs.lock().await,
                                &full_path,
                                Key::File { id: hash },
                            )
                            .await
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    return Err(e);
                                }
                            }
                        } else if is_dir {
                            println!("vfs: zip dir not yet implemented");
                            return Err(VfsError::InternalError);
                        } else {
                            println!("vfs: zip with non-file non-dir");
                            return Err(VfsError::InternalError);
                        };
                    }
                }
            }
            match persist_state(send_to_persist, &mut recv_response, id).await {
                Err(_) => return Err(VfsError::PersistError),
                Ok(_) => return Ok((Some(serde_json::to_string(&VfsResponse::Ok).unwrap()), None)),
            }
        }
        VfsAction::Rename {
            mut full_path,
            mut new_full_path,
        } => {
            full_path = clean_path(&full_path);
            new_full_path = clean_path(&new_full_path);

            match rename_entry(vfs, &full_path, &new_full_path).await {
                Ok(_) => {}
                Err(e) => {
                    return Err(e);
                }
            }

            persist_state(send_to_persist, &mut recv_response, id).await;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Delete(mut full_path) => {
            full_path = clean_path(&full_path);

            let mut vfs = vfs.lock().await;
            let Some(key) = vfs.path_to_key.remove(&full_path) else {
                send_to_terminal
                    .send(Printout {
                        verbosity: 0,
                        content: format!("vfs: can't delete: nonexistent entry {}", full_path),
                    })
                    .await
                    .unwrap();
                return Err(VfsError::EntryNotFound);
            };
            let Some(entry) = vfs.key_to_entry.remove(&key) else {
                send_to_terminal
                    .send(Printout {
                        verbosity: 0,
                        content: format!("vfs: can't delete: nonexistent entry {}", full_path),
                    })
                    .await
                    .unwrap();
                return Err(VfsError::EntryNotFound);
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
                            return Err(VfsError::InternalError);
                        }
                        Some(parent) => {
                            let EntryType::Dir {
                                parent: _,
                                ref mut children,
                            } = parent.entry_type
                            else {
                                return Err(VfsError::InternalError);
                            };
                            //  TODO: does this work?
                            children.remove(&key);
                        }
                    }
                }
            }
            match persist_state(send_to_persist, &mut recv_response, id).await {
                Err(_) => return Err(VfsError::PersistError),
                Ok(_) => return Ok((Some(serde_json::to_string(&VfsResponse::Ok).unwrap()), None)),
            }
        }
        VfsAction::WriteOffset {
            mut full_path,
            offset,
        } => {
            full_path = clean_path(&full_path);

            let file_hash = {
                let mut vfs = vfs.lock().await;
                let Some(key) = vfs.path_to_key.remove(&full_path) else {
                    return Err(VfsError::EntryNotFound);
                };
                let key2 = key.clone();
                let Key::File { id: file_hash } = key2 else {
                    return Err(VfsError::InternalError);
                };
                vfs.path_to_key.insert(full_path.clone(), key);
                file_hash
            };
            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our_node.clone(),
                        process: VFS_PROCESS_ID.clone(),
                    },
                    target: Address {
                        node: our_node.clone(),
                        process: FILESYSTEM_PROCESS_ID.clone(),
                    },
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: true,
                        expects_response: Some(5), // TODO evaluate
                        ipc: serde_json::to_vec(&FsAction::WriteOffset((file_hash, offset)))
                            .unwrap(),
                        metadata: None,
                    }),
                    payload,
                    signed_capabilities: None,
                })
                .await;
            let write_response = recv_response.recv().await.unwrap();
            let KernelMessage { message, .. } = write_response;
            let Message::Response((Response { ipc, .. }, None)) = message else {
                return Err(VfsError::InternalError);
            };
            let Some(ipc) = ipc else {
                return Err(VfsError::InternalError);
            };
            let Ok(FsResponse::Write(_)) =
                serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc).unwrap()
            else {
                return Err(VfsError::InternalError);
            };
            match persist_state(send_to_persist, &mut recv_response, id).await {
                Err(_) => return Err(VfsError::PersistError),
                Ok(_) => return Ok((Some(serde_json::to_string(&VfsResponse::Ok).unwrap()), None)),
            }
        }
        VfsAction::SetSize {
            mut full_path,
            size,
        } => {
            full_path = clean_path(&full_path);

            let file_hash = {
                let mut vfs = vfs.lock().await;
                let Some(key) = vfs.path_to_key.remove(&full_path) else {
                    return Err(VfsError::EntryNotFound);
                };
                let key2 = key.clone();
                let Key::File { id: file_hash } = key2 else {
                    return Err(VfsError::InternalError);
                };
                vfs.path_to_key.insert(full_path.clone(), key);
                file_hash
            };

            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our_node.clone(),
                        process: VFS_PROCESS_ID.clone(),
                    },
                    target: Address {
                        node: our_node.clone(),
                        process: FILESYSTEM_PROCESS_ID.clone(),
                    },
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: true,
                        expects_response: Some(15),
                        ipc: serde_json::to_vec(&FsAction::SetLength((file_hash.clone(), size)))
                            .unwrap(),
                        metadata: None,
                    }),
                    payload: None,
                    signed_capabilities: None,
                })
                .await;
            let write_response = recv_response.recv().await.unwrap();
            let KernelMessage { message, .. } = write_response;
            let Message::Response((Response { ipc, .. }, None)) = message else {
                return Err(VfsError::InternalError);
            };
            let Some(ipc) = ipc else {
                return Err(VfsError::InternalError);
            };
            let Ok(FsResponse::Length(length)) =
                serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc).unwrap()
            else {
                return Err(VfsError::InternalError);
            };
            if length != size {
                return Err(VfsError::InternalError);
            };
            match persist_state(send_to_persist, &mut recv_response, id).await {
                Err(_) => return Err(VfsError::PersistError),
                Ok(_) => return Ok((Some(serde_json::to_string(&VfsResponse::Ok).unwrap()), None)),
            }
        }
        VfsAction::GetPath(hash) => {
            let mut vfs = vfs.lock().await;
            let key = Key::File { id: hash.clone() };
            let ipc =
                serde_json::to_vec(&VfsResponse::GetPath(match vfs.key_to_entry.remove(&key) {
                    None => None,
                    Some(entry) => {
                        let full_path = entry.full_path.clone();
                        vfs.key_to_entry.insert(key, entry);
                        Some(full_path)
                    }
                }))
                .unwrap();
            (ipc, None)
        }
        VfsAction::GetHash(full_path) => {
            let vfs = vfs.lock().await;
            let Some(key) = vfs.path_to_key.get(&full_path) else {
                return Err(VfsError::EntryNotFound);
            };
            let ipc = serde_json::to_vec(&VfsResponse::GetHash(match key {
                Key::File { id } => Some(id.clone()),
                Key::Dir { .. } => None,
            }))
            .unwrap();
            (ipc, None)
        }
        VfsAction::GetEntry(mut full_path) => {
            full_path = clean_path(&full_path);

            let vfs = vfs.lock().await;
            let key = vfs.path_to_key.get(&full_path);
            match key {
                None => return Err(VfsError::EntryNotFound),
                Some(key) => {
                    let entry = vfs.key_to_entry.get(key);
                    match entry {
                        None => return Err(VfsError::EntryNotFound),
                        Some(entry) => match &entry.entry_type {
                            EntryType::Dir { children, .. } => {
                                let paths: Vec<String> = children
                                    .iter()
                                    .filter_map(|child_key| {
                                        vfs.key_to_entry
                                            .get(child_key)
                                            .map(|child| child.full_path.clone())
                                    })
                                    .collect();
                                (
                                    Some(
                                        serde_json::to_string(&VfsResponse::GetEntry {
                                            is_file: false,
                                            children: paths,
                                        })
                                        .unwrap(),
                                    ),
                                    None,
                                )
                            }
                            EntryType::File { parent: _ } => {
                                let Key::File { id: file_hash } = key else {
                                    return Err(VfsError::InternalError);
                                };
                                let _ = send_to_loop
                                    .send(KernelMessage {
                                        id,
                                        source: Address {
                                            node: our_node.clone(),
                                            process: VFS_PROCESS_ID.clone(),
                                        },
                                        target: Address {
                                            node: our_node.clone(),
                                            process: FILESYSTEM_PROCESS_ID.clone(),
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
                                let Message::Response((Response { ipc, .. }, None)) = message
                                else {
                                    return Err(VfsError::InternalError);
                                };
                                let Some(ipc) = ipc else {
                                    return Err(VfsError::InternalError);
                                };
                                let Ok(FsResponse::Read(_read_hash)) =
                                    serde_json::from_str::<Result<FsResponse, FsError>>(&ipc)
                                        .unwrap()
                                else {
                                    println!("vfs: GetEntry fail fs error: {}\r", ipc);
                                    return Err(VfsError::InternalError);
                                };
                                let Some(payload) = payload else {
                                    return Err(VfsError::InternalError);
                                };
                                (
                                    Some(
                                        serde_json::to_string(&VfsResponse::GetEntry {
                                            is_file: true,
                                            children: vec![],
                                        })
                                        .unwrap(),
                                    ),
                                    Some(payload.bytes),
                                )
                            }
                        },
                    }
                }
            }
        }
        VfsAction::GetFileChunk {
            ref full_path,
            offset,
            length,
        } => {
            let file_hash = {
                let mut vfs = vfs.lock().await;
                let Some(key) = vfs.path_to_key.remove(full_path) else {
                    return Err(VfsError::EntryNotFound);
                };
                let key2 = key.clone();
                let Key::File { id: file_hash } = key2 else {
                    return Err(VfsError::InternalError);
                };
                vfs.path_to_key.insert(full_path.clone(), key);
                file_hash
            };

            let _ = send_to_loop
                .send(KernelMessage {
                    id,
                    source: Address {
                        node: our_node.clone(),
                        process: VFS_PROCESS_ID.clone(),
                    },
                    target: Address {
                        node: our_node.clone(),
                        process: FILESYSTEM_PROCESS_ID.clone(),
                    },
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: true,
                        expects_response: Some(5), // TODO evaluate
                        ipc: serde_json::to_vec(&FsAction::ReadChunk(ReadChunkRequest {
                            file: file_hash.clone(),
                            start: offset,
                            length,
                        }))
                        .unwrap(),
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
            let Message::Response((Response { ipc, .. }, None)) = message else {
                return Err(VfsError::InternalError);
            };
            let Some(ipc) = ipc else {
                return Err(VfsError::InternalError);
            };
            let Ok(FsResponse::Read(read_hash)) =
                serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc).unwrap()
            else {
                return Err(VfsError::InternalError);
            };
            assert_eq!(file_hash, read_hash);
            let Some(payload) = payload else {
                return Err(VfsError::InternalError);
            };

            (
                serde_json::to_vec(&VfsResponse::GetFileChunk).unwrap(),
                Some(payload.bytes),
            )
        }
        VfsAction::GetEntryLength(ref full_path) => {
            if full_path.chars().last() == Some('/') {
                (
                    serde_json::to_vec(&VfsResponse::GetEntryLength(None)).unwrap(),
                    None,
                )
            } else {
                let file_hash = {
                    let mut vfs = vfs.lock().await;
                    let Some(key) = vfs.path_to_key.remove(full_path) else {
                        return Err(VfsError::EntryNotFound);
                    };
                    let key2 = key.clone();
                    let Key::File { id: file_hash } = key2 else {
                        return Err(VfsError::InternalError);
                    };
                    vfs.path_to_key.insert(full_path.clone(), key);
                    file_hash
                };

                let _ = send_to_loop
                    .send(KernelMessage {
                        id,
                        source: Address {
                            node: our_node.clone(),
                            process: VFS_PROCESS_ID.clone(),
                        },
                        target: Address {
                            node: our_node.clone(),
                            process: FILESYSTEM_PROCESS_ID.clone(),
                        },
                        rsvp: None,
                        message: Message::Request(Request {
                            inherit: true,
                            expects_response: Some(5), // TODO evaluate
                            ipc: serde_json::to_vec(&FsAction::Length(file_hash)).unwrap(),
                            metadata: None,
                        }),
                        payload: None,
                        signed_capabilities: None,
                    })
                    .await;
                let length_response = recv_response.recv().await.unwrap();
                let KernelMessage { message, .. } = length_response;
                let Message::Response((Response { ipc, .. }, None)) = message else {
                    return Err(VfsError::InternalError);
                };
                let Some(ipc) = ipc else {
                    return Err(VfsError::InternalError);
                };
                let Ok(FsResponse::Length(length)) =
                    serde_json::from_slice::<Result<FsResponse, FsError>>(&ipc).unwrap()
                else {
                    return Err(VfsError::InternalError);
                };

                (
                    serde_json::to_vec(&VfsResponse::GetEntryLength(Some(length))).unwrap(),
                    None,
                )
            }
        }
    })
}
