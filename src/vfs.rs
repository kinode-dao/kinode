use dashmap::DashMap;
use std::collections::{HashMap, VecDeque};
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::Mutex;

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
        return Err(VfsError::BadRequest {
            error: "not a request".into(),
        });
    };

    let request: VfsRequest = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            println!("vfs: got invalid Request: {}", e);
            return Err(VfsError::BadJson {
                error: e.to_string(),
            });
        }
    };

    // current prepend to filepaths needs to be: /package_id/drive/path
    let (package_id, drive, rest) = parse_package_and_drive(&request.path).await?;
    let drive = format!("/{}/{}", package_id, drive);
    let path = PathBuf::from(request.path.clone());


    if km.source.process != *KERNEL_PROCESS_ID {
        check_caps(
            our_node.clone(),
            source.clone(),
            send_to_caps_oracle.clone(),
            &request,
            path.clone(),
            drive.clone(),
            package_id,
            vfs_path.clone(),
        )
        .await?;
    }
    // real safe path that the vfs will use
    let path = PathBuf::from(format!("{}{}/{}", vfs_path, drive, rest));
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
            let _file = open_file(open_files.clone(), path, true).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::OpenFile => {
            let _file = open_file(open_files.clone(), path, false).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CloseFile => {
            open_files.remove(&path);
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::WriteAll => {
            // should expect open file.
            let Some(payload) = payload else {
                return Err(VfsError::BadRequest {
                    error: "payload needs to exist for WriteAll".into(),
                });
            };
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            file.write_all(&payload.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Write => {
            let Some(payload) = payload else {
                return Err(VfsError::BadRequest {
                    error: "payload needs to exist for Write".into(),
                });
            };
            let file = open_file(open_files.clone(), path, true).await?;
            let mut file = file.lock().await;
            file.write_all(&payload.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::WriteAt(offset) => {
            let Some(payload) = payload else {
                return Err(VfsError::BadRequest {
                    error: "payload needs to exist for WriteAt".into(),
                });
            };
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::Start(offset)).await?;
            file.write_all(&payload.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Append => {
            let Some(payload) = payload else {
                return Err(VfsError::BadRequest {
                    error: "payload needs to exist for Append".into(),
                });
            };
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::End(0)).await?;
            file.write_all(&payload.bytes).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::SyncAll => {
            let file = open_file(open_files.clone(), path, false).await?;
            let file = file.lock().await;
            file.sync_all().await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Read => {
            let file = open_file(open_files.clone(), path.clone(), false).await?;
            let mut file = file.lock().await;
            let mut contents = Vec::new();
            file.seek(SeekFrom::Start(0)).await?;
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
                entries.push(entry.path().display().to_string());
            }
            (
                serde_json::to_vec(&VfsResponse::ReadDir(entries)).unwrap(),
                None,
            )
        }
        VfsAction::ReadExact(length) => {
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            let mut contents = vec![0; length as usize];
            file.read_exact(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::Read).unwrap(),
                Some(contents),
            )
        }
        VfsAction::ReadToString => {
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::ReadToString(contents)).unwrap(),
                None,
            )
        }
        VfsAction::Seek(seek_from) => {
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            // same type, rust tingz
            let seek_from = match seek_from {
                crate::types::SeekFrom::Start(offset) => std::io::SeekFrom::Start(offset),
                crate::types::SeekFrom::End(offset) => std::io::SeekFrom::End(offset),
                crate::types::SeekFrom::Current(offset) => std::io::SeekFrom::Current(offset),
            };
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
            let file = open_file(open_files.clone(), path, false).await?;
            let file = file.lock().await;
            let len = file.metadata().await?.len();
            (serde_json::to_vec(&VfsResponse::Len(len)).unwrap(), None)
        }
        VfsAction::SetLen(len) => {
            let file = open_file(open_files.clone(), path, false).await?;
            let file = file.lock().await;
            file.set_len(len).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Hash => {
            let file = open_file(open_files.clone(), path, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::Start(0)).await?;
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0; 1024];
            loop {
                let bytes_read = file.read(&mut buffer).await?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
            let hash: [u8; 32] = hasher.finalize().into();
            (serde_json::to_vec(&VfsResponse::Hash(hash)).unwrap(), None)
        }
        VfsAction::AddZip => {
            let Some(payload) = payload else {
                return Err(VfsError::BadRequest {
                    error: "payload needs to exist for AddZip".into(),
                });
            };
            let Some(mime) = payload.mime else {
                return Err(VfsError::BadRequest {
                    error: "payload mime type needs to exist for AddZip".into(),
                });
            };
            if "application/zip" != mime {
                return Err(VfsError::BadRequest {
                    error: "payload mime type needs to be application/zip for AddZip".into(),
                });
            }
            let file = std::io::Cursor::new(&payload.bytes);
            let mut zip = match zip::ZipArchive::new(file) {
                Ok(f) => f,
                Err(e) => {
                    return Err(VfsError::ParseError {
                        error: e.to_string(),
                        path: path.display().to_string(),
                    })
                }
            };

            // loop through items in archive; recursively add to root
            for i in 0..zip.len() {
                // must destruct the zip file created in zip.by_index()
                //  Before any `.await`s are called since ZipFile is not
                //  Send and so does not play nicely with await
                let (is_file, is_dir, local_path, file_contents) = {
                    let mut file = zip.by_index(i).unwrap();
                    let is_file = file.is_file();
                    let is_dir = file.is_dir();
                    let mut file_contents = Vec::new();
                    if is_file {
                        file.read_to_end(&mut file_contents).unwrap();
                    };
                    let local_path = path.join(file.name());
                    (is_file, is_dir, local_path, file_contents)
                };
                if is_file {
                    let file = open_file(open_files.clone(), local_path, true).await?;
                    let mut file = file.lock().await;
                    file.write_all(&file_contents).await?;
                } else if is_dir {
                    // If it's a directory, create it
                    fs::create_dir_all(local_path).await?;
                } else {
                    println!("vfs: zip with non-file non-dir");
                    return Err(VfsError::CreateDirError {
                        path: path.display().to_string(),
                        error: "vfs: zip with non-file non-dir".into(),
                    });
                };
            }
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
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

async fn parse_package_and_drive(path: &str) -> Result<(PackageId, String, String), VfsError> {
    if !path.starts_with('/') {
        return Err(VfsError::ParseError {
            error: "path does not start with /".into(),
            path: path.to_string(),
        });
    }
    let parts: Vec<&str> = path.split('/').collect();

    let package_id = match PackageId::from_str(parts[1]) {
        Ok(id) => id,
        Err(e) => {
            return Err(VfsError::ParseError {
                error: e.to_string(),
                path: path.to_string(),
            })
        }
    };

    let drive = match parts.get(2) {
        Some(d) => d.to_string(),
        None => {
            return Err(VfsError::ParseError {
                error: "no drive specified".into(),
                path: path.to_string(),
            })
        }
    };

    let remaining_path = parts[3..].join("/");

    Ok((package_id, drive, remaining_path))
}

async fn open_file<P: AsRef<Path>>(
    open_files: Arc<DashMap<PathBuf, Arc<Mutex<fs::File>>>>,
    path: P,
    create: bool,
) -> Result<Arc<Mutex<fs::File>>, VfsError> {
    let path = path.as_ref().to_path_buf();
    Ok(match open_files.get(&path) {
        Some(file) => Arc::clone(file.value()),
        None => {
            let file = Arc::new(Mutex::new(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(create)
                    .open(&path)
                    .await
                    .map_err(|e| VfsError::IOError {
                        error: e.to_string(),
                        path: path.display().to_string(),
                    })?,
            ));
            open_files.insert(path.clone(), Arc::clone(&file));
            file
        }
    })
}

async fn check_caps(
    our_node: String,
    source: Address,
    mut send_to_caps_oracle: CapMessageSender,
    request: &VfsRequest,
    path: PathBuf,
    drive: String,
    package_id: PackageId,
    vfs_dir_path: String,
) -> Result<(), VfsError> {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    match &request.action {
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
        | VfsAction::AddZip
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
                            "drive": drive,
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .await?;
            let has_cap = recv_cap_bool.await?;
            if !has_cap {
                return Err(VfsError::NoCap {
                    action: request.action.to_string(),
                    path: path.display().to_string(),
                });
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
                            "drive": drive,
                        }))
                        .unwrap(),
                    },
                    responder: send_cap_bool,
                })
                .await?;
            let has_cap = recv_cap_bool.await?;
            if !has_cap {
                return Err(VfsError::NoCap {
                    action: request.action.to_string(),
                    path: path.display().to_string(),
                });
            }
            Ok(())
        }
        VfsAction::CreateDrive => {
            // TODO add helper to types.rs?
            let src_package_id =
                PackageId::new(source.process.package(), source.process.publisher());
            if src_package_id != package_id {
                // might have root caps
                send_to_caps_oracle
                    .send(CapMessage::Has {
                        on: source.process.clone(),
                        cap: Capability {
                            issuer: Address {
                                node: our_node.clone(),
                                process: VFS_PROCESS_ID.clone(),
                            },
                            params: serde_json::to_string(&serde_json::json!({
                                "root": true,
                            }))
                            .unwrap(),
                        },
                        responder: send_cap_bool,
                    })
                    .await?;
                let has_cap = recv_cap_bool.await?;
                if !has_cap {
                    return Err(VfsError::NoCap {
                        action: request.action.to_string(),
                        path: path.display().to_string(),
                    });
                }
            }

            add_capability("read", &drive, &our_node, &source, &mut send_to_caps_oracle).await?;
            add_capability(
                "write",
                &drive,
                &our_node,
                &source,
                &mut send_to_caps_oracle,
            )
            .await?;

            let drive_path = format!("{}{}", vfs_dir_path, drive);
            fs::create_dir_all(drive_path).await?;
            Ok(())
        }
    }
}

async fn add_capability(
    kind: &str,
    drive: &str,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &mut CapMessageSender,
) -> Result<(), VfsError> {
    let cap = Capability {
        issuer: Address {
            node: our_node.to_string(),
            process: VFS_PROCESS_ID.clone(),
        },
        params: serde_json::to_string(&serde_json::json!({ "kind": kind, "drive": drive }))
            .unwrap(),
    };
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            cap,
            responder: send_cap_bool,
        })
        .await?;
    let _ = recv_cap_bool.await?;
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

impl From<tokio::sync::oneshot::error::RecvError> for VfsError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        VfsError::CapChannelFail {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for VfsError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        VfsError::CapChannelFail {
            error: err.to_string(),
        }
    }
}

impl std::fmt::Display for VfsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
