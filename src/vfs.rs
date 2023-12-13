use dashmap::DashMap;
use std::collections::{HashMap, VecDeque};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::{Mutex, MutexGuard};

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
        panic!("failed creating vfs dir! {:?}", e);
    }

    //process_A has drive A 
    //process_B creates drive A, conflict. 

    //process_A has drive A, your processId => drive => folder =>

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

                // clone arcs for thread
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
        payload,
        ..
    } = km.clone();
    let Message::Request(Request {
        ipc,
        expects_response,
        metadata,
        ..
    }) = message.clone()
    else {
        return Err(VfsError::BadRequest { error: "not a request".into() })
    };

    let request: VfsRequest = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            println!("vfs: got invalid Request: {}", e);
            return Err(VfsError::BadJson { error: e.to_string() })
        }
    };

    let (process_id, drive) = parse_process_and_drive(&request.path).await?;

    check_caps(
        our_node.clone(),
        source.clone(),
        send_to_caps_oracle.clone(),
        &request,
    )
    .await?;

    let path = request.path; // validate

    let (ipc, bytes) = match request.action {
        VfsAction::CreateDrive => {
            // handled in check_caps.
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CreateDir => {
            // check error mapping
            //     fs::create_dir_all(path).await.map_err(|e| VfsError::IOError { source: e, path: path.clone() })?;
            fs::create_dir(path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CreateDirAll => {
            fs::create_dir_all(path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CreateFile => {
            let file = open_file(open_files.clone(), path).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::OpenFile => {
            // shouldn't create?
            let file = open_file(open_files.clone(), path).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CloseFile => {
            open_files.remove(&path);
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::WriteAll => {
            // should expect open file.
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.write_all(&payload.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Write => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.write_all(&payload.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::WriteAt(offset) => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::Start(offset)).await?;
            file.write_all(&payload.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Append => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::End(0)).await?;
            file.write_all(&payload.bytes).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::SyncAll => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.sync_all().await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Read => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::Read).unwrap(),
                Some(contents),
            )
        }
        VfsAction::ReadDir => {
            let mut dir = fs::read_dir(path).await?;
            let mut entries = Vec::new();
            while let Some(entry) = dir.next_entry().await? {
                // risky non-unicode 
                entries.push(entry.path().to_str()?);
            }
            (
                serde_json::to_vec(&VfsResponse::ReadDir(entries)).unwrap(),
                None
            )
        }
        VfsAction::ReadExact(length) => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            let mut contents = vec![0; length as usize];
            file.read_exact(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::ReadExact).unwrap(),
                Some(contents),
            )
        }
        VfsAction::ReadToString => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::ReadToString).unwrap(),
                Some(contents.into_bytes()),
            )
        }
        VfsAction::Seek(seek_from) => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.seek(seek_from).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::RemoveFile => {
            fs::remove_file(path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::RemoveDir => {
            fs::remove_dir(path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::RemoveDirAll => {
            fs::remove_dir_all(path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Rename(new_path) => {
            // doublecheck permission weirdness, sanitize new path
            fs::rename(path, new_path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Len => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            let len = file.metadata().await?.len();
            (
                serde_json::to_vec(&VfsResponse::Len(Some(len))).unwrap(),
                None,
            )
        }
        VfsAction::SetLen(len) => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            file.set_len(len).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Hash => {
            let file = open_file(open_files.clone(), path).await?;
            let mut file = file.lock().await;
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0; 1024];
            loop {
                let bytes_read = file.read(&mut buffer).await?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
            let hash = hasher.finalize();
            (
                serde_json::to_vec(&VfsResponse::Hash(Some(hash.to_vec()))).unwrap(),
                None,
            )
        }
        VfsAction::AddZip => {
            let Some(mime) = payload.mime else {
                return Err(VfsError::BadRequest { error: "payload mime type needs to exist for AddZip".into() })
            };
            if "application/zip" != mime {
                return Err(VfsError::BadRequest { error: "payload mime type needs to be application/zip for AddZip".into() })
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
                    let file = open_file(open_files.clone(), path).await?;
                    let mut file = file.lock().await;
                    file.write_all(&file_contents).await.unwrap();
                } else if is_dir {
                    let path = validate_path(
                        vfs_path.clone(),
                        request.drive.clone(),
                        path.clone(),
                    )
                    .await?;

                    // If it's a directory, create it
                    fs::create_dir_all(path).await.unwrap();
                } else {
                    println!("vfs: zip with non-file non-dir");
                    return Err(VfsError::CreateDirError { path: path, error: "vfs: zip with non-file non-dir".into() });
                };
            }
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
    }

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

async fn parse_process_and_drive(path: &str) -> Result<(ProcessId, String), VfsError> {
    if !path.starts_with('/') {
        return Err(VfsError::ParseError { error: "path does not start with /".into(), path: path.to_string() }); 
    }
    let parts: Vec<&str> = path.split('/').collect();

    let process_id = match ProcessId::from_str(parts[1]) {
        Ok(id) => id,
        Err(e) => return Err(VfsError::ParseError { error: e.to_string(), path: path.to_string() }),
    };

    let drive = match parts.get(2) {
        Some(d) => d.to_string(),
        None => return Err(VfsError::ParseError { error: "no drive specified".into(), path: path.to_string() }),
    };
    
    Ok((process_id, drive))
}

async fn validate_path(
    vfs_path: String,
    drive: String,
    request_path: String,
) -> Result<PathBuf, VfsError> {
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
    // process_id + drive. + kernel needs auto_access. 
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
                            "process_id": process_id.to_string(),
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

async fn check_capss(
    our_node: String,
    source: Address,
    send_to_caps_oracle: CapMessageSender,
    request: &VfsRequest,
) -> Result<(), VfsError> {
    match request.action {
        VfsAction::CreateDir
        | VfsAction::CreateDirAll
        | VfsAction::CreateFile
        | VfsAction::OpenFile
        | VfsAction::CloseFile
        | VfsAction::WriteAll
        | VfsAction::Write
        | VfsAction::WriteAt(_)
        | VfsAction::Append
        | VfsAction::SyncAll
        | VfsAction::RemoveFile
        | VfsAction::RemoveDir
        | VfsAction::RemoveDirAll
        | VfsAction::Rename(_)
        | VfsAction::SetLen(_) => {
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
                            "process_id": process_id.to_string(),
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
        VfsAction::Read
        | VfsAction::ReadDir
        | VfsAction::ReadExact(_)
        | VfsAction::ReadToString
        | VfsAction::Seek(_)
        | VfsAction::Hash
        | VfsAction::Len => {
            if !caps.read {
                return Err(VfsError::NoCap {
                    action: format!("{:?}", action),
                    path: caps.path.clone(),
                });
            }
        }
        VfsAction::CreateDrive {

        }
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
                ipc: serde_json::to_vec(&VfsResponse::Err(error)).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
