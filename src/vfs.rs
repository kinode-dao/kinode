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

    let mut process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::new();

    loop {
        tokio::select! {
            Some(km) = recv_from_loop.recv() => {
                if our_node.clone() != km.source.node {
                    println!(
                        "vfs: request must come from our_node={}, got: {}",
                        our_node,
                        km.source.node,
                    );
                    continue;
                }

                let queue = process_queues
                    .entry(km.source.process.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(VecDeque::new())))
                    .clone();

                {
                    let mut queue_lock = queue.lock().await;
                    queue_lock.push_back(km.clone());
                }

                // clone Arcs
                let our_node = our_node.clone();
                let send_to_caps_oracle = send_to_caps_oracle.clone();
                let send_to_terminal = send_to_terminal.clone();
                let send_to_loop = send_to_loop.clone();
                let open_files = open_files.clone();
                let vfs_path = vfs_path.clone();

                tokio::spawn(async move {
                    let mut queue_lock = queue.lock().await;
                    if let Some(km) = queue_lock.pop_front() {
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
        lazy_load_blob: blob,
        ..
    } = km.clone();
    let Message::Request(Request {
        body,
        expects_response,
        metadata,
        ..
    }) = message.clone()
    else {
        return Err(VfsError::BadRequest {
            error: "not a request".into(),
        });
    };

    let request: VfsRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            println!("vfs: got invalid Request: {}", e);
            return Err(VfsError::BadJson {
                error: e.to_string(),
            });
        }
    };

    // special case for root reading list of all drives.
    if request.action == VfsAction::ReadDir && request.path == "/" {
        // check if src has root
        let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
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
        let has_root_cap = recv_cap_bool.await?;
        if has_root_cap {
            let mut dir = fs::read_dir(vfs_path.clone()).await?;
            let mut entries = Vec::new();
            while let Some(entry) = dir.next_entry().await? {
                let entry_path = entry.path();
                let relative_path = entry_path.strip_prefix(&vfs_path).unwrap_or(&entry_path);

                let metadata = entry.metadata().await?;
                let file_type = get_file_type(&metadata);
                let dir_entry = DirEntry {
                    path: relative_path.display().to_string(),
                    file_type,
                };
                entries.push(dir_entry);
            }

            let response = KernelMessage {
                id,
                source: Address {
                    node: our_node.clone(),
                    process: VFS_PROCESS_ID.clone(),
                },
                target: source,
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        body: serde_json::to_vec(&VfsResponse::ReadDir(entries)).unwrap(),
                        metadata,
                        capabilities: vec![],
                    },
                    None,
                )),
                lazy_load_blob: None,
            };

            let _ = send_to_loop.send(response).await;
            return Ok(());
        } else {
            let no_cap_error = VfsError::NoCap {
                action: request.action.to_string(),
                path: request.path.clone(),
            };
            return Err(no_cap_error);
        }
    }

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
    let (body, bytes) = match request.action {
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
            // create truncates any file that might've existed before
            open_files.remove(&path);
            let _file = open_file(open_files.clone(), path, true, true).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::OpenFile { create } => {
            // open file opens an existing file, or creates a new one if create is true
            let file = open_file(open_files.clone(), path, create, false).await?;
            let mut file = file.lock().await;
            // extra in the case file was just created, todo refactor out.
            file.seek(SeekFrom::Start(0)).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CloseFile => {
            // removes file from scope, resets file_handle and cursor.
            open_files.remove(&path);
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::WriteAll => {
            // doesn't create a file, writes at exact cursor.
            let Some(blob) = blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for WriteAll".into(),
                });
            };
            let file = open_file(open_files.clone(), path, false, false).await?;
            let mut file = file.lock().await;
            file.write_all(&blob.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Write => {
            let Some(blob) = blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for Write".into(),
                });
            };
            fs::write(path, &blob.bytes).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Append => {
            let Some(blob) = blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for Append".into(),
                });
            };
            let file = open_file(open_files.clone(), path, false, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::End(0)).await?;
            file.write_all(&blob.bytes).await?;

            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::SyncAll => {
            let file = open_file(open_files.clone(), path, false, false).await?;
            let file = file.lock().await;
            file.sync_all().await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Read => {
            let contents = fs::read(&path).await?;
            (
                serde_json::to_vec(&VfsResponse::Read).unwrap(),
                Some(contents),
            )
        }
        VfsAction::ReadToEnd => {
            let file = open_file(open_files.clone(), path.clone(), false, false).await?;
            let mut file = file.lock().await;
            let mut contents = Vec::new();

            file.read_to_end(&mut contents).await?;

            (
                serde_json::to_vec(&VfsResponse::Read).unwrap(),
                Some(contents),
            )
        }
        VfsAction::ReadExact(length) => {
            let file = open_file(open_files.clone(), path, false, false).await?;
            let mut file = file.lock().await;
            let mut contents = vec![0; length as usize];
            file.read_exact(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::Read).unwrap(),
                Some(contents),
            )
        }
        VfsAction::ReadDir => {
            let mut dir = fs::read_dir(path).await?;
            let mut entries = Vec::new();
            while let Some(entry) = dir.next_entry().await? {
                let entry_path = entry.path();
                let relative_path = entry_path.strip_prefix(&vfs_path).unwrap_or(&entry_path);

                let metadata = entry.metadata().await?;
                let file_type = get_file_type(&metadata);
                let dir_entry = DirEntry {
                    path: relative_path.display().to_string(),
                    file_type,
                };
                entries.push(dir_entry);
            }
            (
                serde_json::to_vec(&VfsResponse::ReadDir(entries)).unwrap(),
                None,
            )
        }
        VfsAction::ReadToString => {
            let file = open_file(open_files.clone(), path, false, false).await?;
            let mut file = file.lock().await;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;
            (
                serde_json::to_vec(&VfsResponse::ReadToString(contents)).unwrap(),
                None,
            )
        }
        VfsAction::Seek { seek_from } => {
            let file = open_file(open_files.clone(), path, false, false).await?;
            let mut file = file.lock().await;
            // same type, rust tingz
            let seek_from = match seek_from {
                crate::types::SeekFrom::Start(offset) => std::io::SeekFrom::Start(offset),
                crate::types::SeekFrom::End(offset) => std::io::SeekFrom::End(offset),
                crate::types::SeekFrom::Current(offset) => std::io::SeekFrom::Current(offset),
            };
            let response = file.seek(seek_from).await?;
            (
                serde_json::to_vec(&VfsResponse::SeekFrom(response)).unwrap(),
                None,
            )
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
        VfsAction::Rename { new_path } => {
            fs::rename(path, new_path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::CopyFile { new_path } => {
            fs::copy(path, new_path).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Metadata => {
            let metadata = fs::metadata(&path).await?;

            let file_type = get_file_type(&metadata);
            let meta = FileMetadata {
                len: metadata.len(),
                file_type,
            };

            (
                serde_json::to_vec(&VfsResponse::Metadata(meta)).unwrap(),
                None,
            )
        }
        VfsAction::Len => {
            let file = open_file(open_files.clone(), path, false, false).await?;
            let file = file.lock().await;
            let len = file.metadata().await?.len();
            (serde_json::to_vec(&VfsResponse::Len(len)).unwrap(), None)
        }
        VfsAction::SetLen(len) => {
            let file = open_file(open_files.clone(), path, false, false).await?;
            let file = file.lock().await;
            file.set_len(len).await?;
            (serde_json::to_vec(&VfsResponse::Ok).unwrap(), None)
        }
        VfsAction::Hash => {
            let file = open_file(open_files.clone(), path, false, false).await?;
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
            let Some(blob) = blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for AddZip".into(),
                });
            };
            let Some(mime) = blob.mime else {
                return Err(VfsError::BadRequest {
                    error: "blob mime type needs to exist for AddZip".into(),
                });
            };
            if "application/zip" != mime {
                return Err(VfsError::BadRequest {
                    error: "blob mime type needs to be application/zip for AddZip".into(),
                });
            }
            let file = std::io::Cursor::new(&blob.bytes);
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
                    let mut file = zip.by_index(i)?;
                    let is_file = file.is_file();
                    let is_dir = file.is_dir();
                    let mut file_contents = Vec::new();
                    if is_file {
                        file.read_to_end(&mut file_contents)?;
                    };
                    let local_path = path.join(file.name());
                    (is_file, is_dir, local_path, file_contents)
                };
                if is_file {
                    fs::write(&local_path, &file_contents).await?;
                } else if is_dir {
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
                    body,
                    metadata,
                    capabilities: vec![],
                },
                None,
            )),
            lazy_load_blob: bytes.map(|bytes| LazyLoadBlob {
                mime: Some("application/octet-stream".into()),
                bytes,
            }),
        };

        let _ = send_to_loop.send(response).await;
    } else {
        println!("vfs: not sending response: ");
        send_to_terminal
            .send(Printout {
                verbosity: 2,
                content: format!(
                    "vfs: not sending response: {:?}",
                    serde_json::from_slice::<VfsResponse>(&body)
                ),
            })
            .await
            .unwrap();
    }

    Ok(())
}

async fn parse_package_and_drive(path: &str) -> Result<(PackageId, String, String), VfsError> {
    let mut parts: Vec<&str> = path.split('/').collect();

    if parts[0].is_empty() {
        parts.remove(0);
    }
    if parts.len() < 2 {
        return Err(VfsError::ParseError {
            error: "malformed path".into(),
            path: path.to_string(),
        });
    }

    let package_id = match parts[0].parse::<PackageId>() {
        Ok(id) => id,
        Err(e) => {
            return Err(VfsError::ParseError {
                error: e.to_string(),
                path: path.to_string(),
            })
        }
    };

    let drive = parts[1].to_string();
    let remaining_path = parts[2..].join("/");

    Ok((package_id, drive, remaining_path))
}

async fn open_file<P: AsRef<Path>>(
    open_files: Arc<DashMap<PathBuf, Arc<Mutex<fs::File>>>>,
    path: P,
    create: bool,
    truncate: bool,
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
                    .truncate(truncate)
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
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    // check for root cap (todo make temp buffer so this is more efficient?)
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
    let has_root_cap = recv_cap_bool.await?;

    match &request.action {
        VfsAction::CreateDir
        | VfsAction::CreateDirAll
        | VfsAction::CreateFile
        | VfsAction::OpenFile { .. }
        | VfsAction::CloseFile
        | VfsAction::Write
        | VfsAction::WriteAll
        | VfsAction::Append
        | VfsAction::SyncAll
        | VfsAction::RemoveFile
        | VfsAction::RemoveDir
        | VfsAction::RemoveDirAll
        | VfsAction::AddZip
        | VfsAction::SetLen(_) => {
            if src_package_id == package_id {
                return Ok(());
            }

            if has_root_cap {
                return Ok(());
            }
            let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
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
        | VfsAction::ReadToEnd
        | VfsAction::ReadToString
        | VfsAction::Seek { .. }
        | VfsAction::Hash
        | VfsAction::Metadata
        | VfsAction::Len => {
            if src_package_id == package_id {
                return Ok(());
            }
            if has_root_cap {
                return Ok(());
            }
            let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
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
        VfsAction::CopyFile { new_path } | VfsAction::Rename { new_path } => {
            // these have 2 paths to validate
            if has_root_cap {
                return Ok(());
            }

            let (new_package_id, new_drive, _rest) = parse_package_and_drive(new_path).await?;
            let new_drive = format!("/{}/{}", new_package_id, new_drive);
            // if both new and old path are within the package_id path, ok
            if (src_package_id == package_id) && (src_package_id == new_package_id) {
                return Ok(());
            }

            // otherwise check write caps.
            let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
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

            // if they're within the same drive, no need for 2 caps checks
            if new_drive == drive {
                return Ok(());
            }

            let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
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
                            "drive": new_drive,
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
            if src_package_id != package_id && !has_root_cap {
                return Err(VfsError::NoCap {
                    action: request.action.to_string(),
                    path: path.display().to_string(),
                });
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
            caps: vec![cap],
            responder: send_cap_bool,
        })
        .await?;
    let _ = recv_cap_bool.await?;
    Ok(())
}

fn get_file_type(metadata: &std::fs::Metadata) -> FileType {
    if metadata.is_file() {
        FileType::File
    } else if metadata.is_dir() {
        FileType::Directory
    } else if metadata.file_type().is_symlink() {
        FileType::Symlink
    } else {
        FileType::Other
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
                body: serde_json::to_vec(&VfsResponse::Err(error)).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: None,
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

impl From<zip::result::ZipError> for VfsError {
    fn from(err: zip::result::ZipError) -> Self {
        VfsError::IOError {
            error: err.to_string(),
            path: "".into(),
        }
    }
}

impl From<std::io::Error> for VfsError {
    fn from(err: std::io::Error) -> Self {
        VfsError::IOError {
            path: "".into(),
            error: err.to_string(),
        }
    }
}

impl std::fmt::Display for VfsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
