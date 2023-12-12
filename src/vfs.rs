use std::collections::{HashMap, VecDeque};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use dashmap::DashMap;

use crate::types::*;

pub async fn vfs(
    our_node: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: String,
) -> anyhow::Result<()> {
    let vfs_path = format!("{}/vfs", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&vfs_path).await {
        panic!("failed creating fs dir! {:?}", e);
    }

    let open_files: Arc<DashMap<PathBuf, Arc<Mutex<fs::File>>>> = Arc::new(DashMap::new());
    let process_queues = Arc::new(Mutex::new(
        HashMap::<ProcessId, VecDeque<KernelMessage>>::new(),
    ));    
    // note: queues should be based on drive, not process
    loop {
        tokio::select! {
            Some(km) = recv_from_loop.recv() => {
                if our_node != km.source.node {
                    println!(
                        "vfs: request must come from our_node={}, got: {}",
                        our_node,
                        km.source.node,
                    );
                    continue;
                }
    
                // clone Arcs for thread
                let our_node = our_node.clone();
                let send_to_caps_oracle = send_to_caps_oracle.clone();
                let send_to_terminal = send_to_terminal.clone();
                let send_to_loop = send_to_loop.clone();
                let open_files = open_files.clone();
                let vfs_path = vfs_path.clone();
    
                let mut process_lock = process_queues.lock().await;
    
                if let Some(queue) = process_lock.get_mut(&km.source.process) {
                    queue.push_back(km.clone());
                } else {
                    let mut new_queue = VecDeque::new();
                    new_queue.push_back(km.clone());
                    process_lock.insert(km.source.process.clone(), new_queue);
                }
    
                // clone Arc for thread
                let process_queues_clone = process_queues.clone();
    
                tokio::spawn(async move {
                    let mut process_lock = process_queues_clone.lock().await;
                    if let Some(km) = process_lock.get_mut(&km.source.process).and_then(|q| q.pop_front()) {
                        if let Err(e) = handle_request(
                            our_node.clone(),
                            km.clone(),
                            open_files.clone(),
                            send_to_loop.clone(),
                            send_to_terminal.clone(),
                            send_to_caps_oracle.clone(),
                            vfs_path.clone(),
                        )
                        .await
                        {
                            let _ = send_to_loop
                                .send(make_error_message(our_node.clone(), km.id, km.source, e))
                                .await;
                        }
                    }
                    // Remove the process entry if no more tasks are left
                    if let Some(queue) = process_lock.get(&km.source.process) {
                        if queue.is_empty() {
                            process_lock.remove(&km.source.process);
                        }
                    }
                });
            }
        }
    }
}

async fn handle_request(
    our_node: String,
    km: KernelMessage,
    open_files: Arc<DashMap<PathBuf, Arc<Mutex<fs::File>>>>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    send_to_caps_oracle: CapMessageSender,
    vfs_path: String,
) -> Result<(), VfsError> {
    let KernelMessage {
        id,
        source,
        message,
        ..
    } = km.clone();
    let Message::Request(Request {
        ipc,
        expects_response,
        metadata,
        inherit,
    }) = message.clone()
    else {
        return Err(VfsError::BadJson);
    };

    let request: VfsRequest = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            println!("vfs: got invalid Request: {}", e);
            return Err(VfsError::BadJson);
        }
    };

    check_caps(our_node.clone(), source.clone(), send_to_caps_oracle.clone(), &request)
        .await?;

    let (ipc, bytes) = match request.action {
        VfsAction::New => {
            // handled in check_caps
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Add {
            mut full_path,
            entry_type,
        } => {
            match entry_type {
                AddEntryType::Dir => {
                    let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;

                    // create dir.
                    fs::create_dir_all(path).await.unwrap();
                }
                AddEntryType::NewFile => {
                    let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
                    // open and create file
                    let file = open_file(open_files.clone(), path).await?;
                    let mut file = file.lock().await;
                    file.write_all(&km.payload.unwrap().bytes).await.unwrap();
                }
                AddEntryType::ZipArchive => {
                    let Some(payload) = km.payload else {
                        return Err(VfsError::BadPayload);
                    };
                    let Some(mime) = payload.mime else {
                        return Err(VfsError::BadPayload);
                    };
                    if "application/zip" != mime {
                        return Err(VfsError::BadPayload);
                    }
                    let file = std::io::Cursor::new(&payload.bytes);
                    let mut zip = match zip::ZipArchive::new(file) {
                        Ok(f) => f,
                        Err(_) => return Err(VfsError::InternalError),
                    };

                    // loop through items in archive; recursively add to root
                    for i in 0..zip.len() {
                        // must destruct the zip file created in zip.by_index()
                        //  Before any `.await`s are called since ZipFile is not
                        //  Send and so does not play nicely with await
                        let (is_file, is_dir, path, file_contents) = {
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
                            // create file
                            println!("writing a file!, orig filename {:?}", path);
                            let path = validate_path(vfs_path.clone(), request.drive.clone(), path).await?;
                            println!("with the path: {:?}", path);
                            println!("and original path: {:?}", full_path);
                            let file = open_file(open_files.clone(), path).await?;
                            println!("opening file!:");
                            let mut file = file.lock().await;
                            file.write_all(&file_contents).await.unwrap();
                            println!("actually wrote file!");
        
                        } else if is_dir {
                            let path = validate_path(vfs_path.clone(), request.drive.clone(), path.clone()).await?;

                            // If it's a directory, create it
                            fs::create_dir_all(path).await.unwrap();
                        } else {
                            println!("vfs: zip with non-file non-dir");
                            return Err(VfsError::InternalError);
                        };
                    }
                }
            }
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Delete(mut full_path) => {
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::WriteOffset {
            mut full_path,
            offset,
        } => {
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::Start(offset)).await.unwrap();
            file.write_all(&km.payload.unwrap().bytes).await.unwrap();
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Append(mut full_path) => {
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::End(0)).await.unwrap();
            file.write_all(&km.payload.unwrap().bytes).await.unwrap();

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::SetSize {
            mut full_path,
            size,
        } => {
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.set_len(size).await.unwrap();

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::GetEntry(mut full_path) => {
            println!("getting entry for path: {:?}", full_path);
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
            println!("getentry path resolved to: {:?}", path);
            let metadata = fs::metadata(&path).await.unwrap();
            if metadata.is_dir() {
                let mut children = Vec::new();
                let mut entries = fs::read_dir(&path).await.unwrap();
            
                while let Some(entry) = entries.next_entry().await.unwrap() {
                    children.push(entry.path().display().to_string());
                }

                (serde_json::to_vec(&VfsResponse::GetEntry { is_file: false, children }).unwrap(), None)
            } else if metadata.is_file() {
                println!("is file!");
                let file = open_file(open_files.clone(), path).await?;
                let mut file = file.lock().await;
                println!("got file lock.");
                let mut contents = Vec::new();
                file.read_to_end(&mut contents).await.unwrap();
                println!("read contents to last part");
                (serde_json::to_vec(&VfsResponse::GetEntry { is_file: true, children: Vec::new() }).unwrap(), Some(contents))

            } else {
                return Err(VfsError::InternalError)
            }
        }
        VfsAction::GetFileChunk {
            mut full_path,
            offset,
            length,
        } => {
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            let mut contents = vec![0; length as usize];

            file.seek(SeekFrom::Start(offset)).await.unwrap();
            file.read_exact(&mut contents).await.unwrap();
                    (
                serde_json::to_vec(&VfsResponse::GetFileChunk).unwrap(),
                Some(contents),
            )
        }
        VfsAction::GetEntryLength(mut full_path) => {
            let path = validate_path(vfs_path.clone(), request.drive.clone(), full_path.clone()).await?;
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;

            let length = file.metadata().await.unwrap().len();

                (
                    serde_json::to_vec(&VfsResponse::GetEntryLength(Some(length))).unwrap(),
                    None,
                )
            }
        };

    if let Some(target) = km.rsvp.or_else(|| {
        expects_response.map(|_| Address {
            node: our_node.clone(),
            process: source.process.clone(),
        })
    }) {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_node.clone(),
                process: VFS_PROCESS_ID.clone(),
            },
            target,
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc,
                    metadata,
                },
                None,
            )),
            payload: bytes.map(|bytes| Payload {
                mime: Some("application/octet-stream".into()),
                bytes,
            }),
            signed_capabilities: None,
        };

        let _ = send_to_loop.send(response).await;
    } else {
        println!("vfs: not sending response: ");
        send_to_terminal
            .send(Printout {
                verbosity: 2,
                content: format!(
                    "vfs: not sending response: {:?}",
                    serde_json::from_slice::<VfsResponse>(&ipc)
                ),
            })
            .await
            .unwrap();
    }

    Ok(())
}

async fn validate_path(vfs_path: String, drive: String, request_path: String) -> Result<PathBuf, VfsError> {
    let drive_base = Path::new(&vfs_path).join(&drive);
    if let Err(e) = fs::create_dir_all(&drive_base).await {
        println!("failed creating drive dir! {:?}", e);
    }
    let request_path = request_path.strip_prefix("/").unwrap_or(&request_path);

    let combined = drive_base.join(&request_path);
    if combined.starts_with(&drive_base) {
        Ok(combined)
    } else {
        println!("didn't start with base, combined: {:?}", combined);
        Err(VfsError::InternalError)
    }
}

async fn open_file(
    open_files: Arc<DashMap<PathBuf, Arc<Mutex<fs::File>>>>,
    path: PathBuf,
) -> Result<Arc<Mutex<fs::File>>, VfsError> {
    Ok(match open_files.get(&path) {
        Some(file) => Arc::clone(file.value()),
        None => {
            let file = Arc::new(Mutex::new(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .open(&path)
                    .await
                    .map_err(|_| VfsError::InternalError)?,
            ));
            open_files.insert(path, Arc::clone(&file));
            file
        }
    })
}

async fn check_caps(
    our_node: String,
    source: Address,
    send_to_caps_oracle: CapMessageSender,
    request: &VfsRequest,
) -> Result<(), VfsError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    //  check caps
    match &request.action {
        VfsAction::Add { .. }
        | VfsAction::Delete { .. }
        | VfsAction::WriteOffset { .. }
        | VfsAction::Append { .. }
        | VfsAction::SetSize { .. } => {
            send_to_caps_oracle
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
            Ok(())
        }
        VfsAction::GetEntry { .. }
        | VfsAction::GetFileChunk { .. }
        | VfsAction::GetEntryLength { .. } => {
            send_to_caps_oracle
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
            Ok(())
        }
        VfsAction::New { .. } => {
            let read_cap = Capability {
                issuer: Address {
                    node: our_node.clone(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::to_string(
                    &serde_json::json!({"kind": "read", "drive": request.drive}),
                )
                .unwrap(),
            };
            let write_cap = Capability {
                issuer: Address {
                    node: our_node.clone(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::to_string(
                    &serde_json::json!({"kind": "write", "drive": request.drive}),
                )
                .unwrap(),
            };
            let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
            send_to_caps_oracle
                .send(CapMessage::Add {
                    on: source.process.clone(),
                    cap: read_cap,
                    responder: send_cap_bool,
                })
                .await
                .unwrap();
            let _ = recv_cap_bool.await.unwrap();
            let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
            send_to_caps_oracle
                .send(CapMessage::Add {
                    on: source.process.clone(),
                    cap: write_cap,
                    responder: send_cap_bool,
                })
                .await
                .unwrap();
            let _ = recv_cap_bool.await.unwrap();
            Ok(())
        }
    }
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
                ipc: serde_json::to_vec(&VfsResponse::Err(error)).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}