use crate::filesystem::manifest::{FileIdentifier, Manifest};
use crate::types::*;
/// log structured filesystem
use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::oneshot::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
mod manifest;

pub async fn load_fs(
    our_name: String,
    home_directory_path: String,
    file_key: Vec<u8>,
    fs_config: FsConfig,
) -> Result<(ProcessMap, Manifest, Vec<KernelMessage>), FsError> {
    // load/create fs directory, manifest + log if none.
    let fs_directory_path_str = format!("{}/fs", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&fs_directory_path_str).await {
        panic!("failed creating fs dir! {:?}", e);
    }

    let fs_directory_path: std::path::PathBuf =
        fs::canonicalize(fs_directory_path_str).await.unwrap();

    //  open and load manifest+log

    let manifest_path = fs_directory_path.join("manifest.bin");

    let manifest_file = fs::OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .open(&manifest_path)
        .await
        .expect("fs: failed to open manifest file");

    let wal_path = fs_directory_path.join("wal.bin");

    let wal_file = fs::OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .open(&wal_path)
        .await
        .expect("fs: failed to open WAL file");

    //  in memory details about files.
    let mut manifest = Manifest::load(
        manifest_file,
        wal_file,
        &fs_directory_path,
        file_key,
        fs_config,
    )
    .await
    .expect("manifest load failed!");

    // get kernel state for booting up
    let kernel_process_id = FileIdentifier::Process(KERNEL_PROCESS_ID.clone());
    let mut process_map: ProcessMap = HashMap::new();

    // get current processes' wasm_bytes handles. GetState(kernel)
    let vfs_messages = match manifest.read(&kernel_process_id, None, None).await {
        Err(_) => {
            //  bootstrap filesystem
            bootstrap(
                &our_name,
                &kernel_process_id,
                &mut process_map,
                &mut manifest,
            )
            .await
            .expect("fresh bootstrap failed!")
        }
        Ok(bytes) => {
            process_map = bincode::deserialize(&bytes).expect("state map deserialization error!");
            vec![]
        }
    };

    Ok((process_map, manifest, vfs_messages))
}

/// function run only upon fresh boot.
///
/// for each folder in /modules, looks for a package.zip file, extracts the contents,
/// sends the contents to VFS, and reads the manifest.json.
///
/// the manifest.json contains instructions for which processes to boot and what
/// capabilities to give them. since we are inside runtime, can spawn those out of
/// thin air.
async fn bootstrap(
    our_name: &str,
    kernel_process_id: &FileIdentifier,
    process_map: &mut ProcessMap,
    manifest: &mut Manifest,
) -> Result<Vec<KernelMessage>> {
    println!("bootstrapping node...\r");
    const RUNTIME_MODULES: [(&str, bool); 8] = [
        ("filesystem:sys:uqbar", false),
        ("http_server:sys:uqbar", true), // TODO evaluate
        ("http_client:sys:uqbar", false),
        ("encryptor:sys:uqbar", false),
        ("net:sys:uqbar", false),
        ("vfs:sys:uqbar", true),
        ("kernel:sys:uqbar", false),
        ("eth_rpc:sys:uqbar", true), // TODO evaluate
    ];

    let mut runtime_caps: HashSet<Capability> = HashSet::new();
    for runtime_module in RUNTIME_MODULES {
        runtime_caps.insert(Capability {
            issuer: Address {
                node: our_name.to_string(),
                process: ProcessId::from_str(runtime_module.0).unwrap(),
            },
            params: "\"messaging\"".into(),
        });
    }
    // give all runtime processes the ability to send messages across the network
    runtime_caps.insert(Capability {
        issuer: Address {
            node: our_name.to_string(),
            process: KERNEL_PROCESS_ID.clone(),
        },
        params: "\"network\"".into(),
    });

    // finally, save runtime modules in state map as well, somewhat fakely
    for runtime_module in RUNTIME_MODULES {
        process_map
            .entry(ProcessId::from_str(runtime_module.0).unwrap())
            .or_insert(PersistedProcess {
                wasm_bytes_handle: 0,
                on_panic: OnPanic::Restart,
                capabilities: runtime_caps.clone(),
                public: runtime_module.1,
            });
    }

    let packages: Vec<(String, zip::ZipArchive<std::io::Cursor<Vec<u8>>>)> =
        get_zipped_packages().await;

    // need to grant all caps at the end, after process_map has been filled in!
    let mut caps_to_grant = Vec::<(ProcessId, Capability)>::new();

    let mut vfs_messages = Vec::new();

    for (package_name, mut package) in packages {
        println!("fs: handling package {package_name}...\r");
        // create a new package in VFS
        vfs_messages.push(KernelMessage {
            id: rand::random(),
            source: Address {
                node: our_name.to_string(),
                process: FILESYSTEM_PROCESS_ID.clone(),
            },
            target: Address {
                node: our_name.to_string(),
                process: VFS_PROCESS_ID.clone(),
            },
            rsvp: None,
            message: Message::Request(Request {
                inherit: false,
                expects_response: None,
                ipc: Some(
                    serde_json::to_string::<VfsRequest>(&VfsRequest {
                        drive: package_name.clone(),
                        action: VfsAction::New,
                    })
                    .unwrap(),
                ),
                metadata: None,
            }),
            payload: None,
            signed_capabilities: None,
        });
        // for each file in package.zip, recursively through all dirs, send a newfile KM to VFS
        for i in 0..package.len() {
            let mut file = package.by_index(i).unwrap();
            if file.is_file() {
                let file_path = file
                    .enclosed_name()
                    .expect("fs: name error reading package.zip")
                    .to_owned();
                let mut file_path = file_path.to_string_lossy().to_string();
                if !file_path.starts_with("/") {
                    file_path = format!("/{}", file_path);
                }
                println!("fs: found file {}...\r", file_path);
                let mut file_content = Vec::new();
                file.read_to_end(&mut file_content).unwrap();
                vfs_messages.push(KernelMessage {
                    id: rand::random(),
                    source: Address {
                        node: our_name.to_string(),
                        process: FILESYSTEM_PROCESS_ID.clone(),
                    },
                    target: Address {
                        node: our_name.to_string(),
                        process: VFS_PROCESS_ID.clone(),
                    },
                    rsvp: None,
                    message: Message::Request(Request {
                        inherit: false,
                        expects_response: None,
                        ipc: Some(
                            serde_json::to_string::<VfsRequest>(&VfsRequest {
                                drive: package_name.clone(),
                                action: VfsAction::Add {
                                    full_path: file_path,
                                    entry_type: AddEntryType::NewFile,
                                },
                            })
                            .unwrap(),
                        ),
                        metadata: None,
                    }),
                    payload: Some(Payload {
                        mime: None,
                        bytes: file_content,
                    }),
                    signed_capabilities: None,
                });
            }
        }

        // get and read metadata.json
        let Ok(mut package_metadata_zip) = package.by_name("metadata.json") else {
            println!(
                "fs: missing metadata for package {}, skipping",
                package_name
            );
            continue;
        };
        let mut metadata_content = Vec::new();
        package_metadata_zip
            .read_to_end(&mut metadata_content)
            .unwrap();
        drop(package_metadata_zip);
        let package_metadata: serde_json::Value =
            serde_json::from_slice(&metadata_content).expect("fs: metadata parse error");

        println!("fs: found package metadata: {:?}\r", package_metadata);

        let package_name = package_metadata["package"]
            .as_str()
            .expect("fs: metadata parse error: bad package name");

        let package_publisher = package_metadata["publisher"]
            .as_str()
            .expect("fs: metadata parse error: bad publisher name");

        // get and read manifest.json
        let Ok(mut package_manifest_zip) = package.by_name("manifest.json") else {
            println!(
                "fs: missing manifest for package {}, skipping",
                package_name
            );
            continue;
        };
        let mut manifest_content = Vec::new();
        package_manifest_zip
            .read_to_end(&mut manifest_content)
            .unwrap();
        drop(package_manifest_zip);
        let package_manifest = String::from_utf8(manifest_content)?;
        let package_manifest = serde_json::from_str::<Vec<PackageManifestEntry>>(&package_manifest)
            .expect("fs: manifest parse error");

        // for each process-entry in manifest.json:
        for mut entry in package_manifest {
            let wasm_bytes = &mut Vec::new();
            let mut file_path = format!("{}", entry.process_wasm_path);
            if file_path.starts_with("/") {
                file_path = format!("{}", &file_path[1..]);
            }
            package
                .by_name(&file_path)
                .expect("fs: no wasm found in package!")
                .read_to_end(wasm_bytes)
                .unwrap();

            // spawn the requested capabilities
            // remember: out of thin air, because this is the root distro
            let mut requested_caps = HashSet::new();
            let our_process_id = format!(
                "{}:{}:{}",
                entry.process_name, package_name, package_publisher
            );
            entry.request_messaging.push(our_process_id.clone());
            for process_name in &entry.request_messaging {
                requested_caps.insert(Capability {
                    issuer: Address {
                        node: our_name.to_string(),
                        process: ProcessId::from_str(process_name).unwrap(),
                    },
                    params: "\"messaging\"".into(),
                });
            }

            if entry.request_networking {
                requested_caps.insert(Capability {
                    issuer: Address {
                        node: our_name.to_string(),
                        process: KERNEL_PROCESS_ID.clone(),
                    },
                    params: "\"network\"".into(),
                });
            }

            // give access to package_name vfs
            requested_caps.insert(Capability {
                issuer: Address {
                    node: our_name.into(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::to_string(&serde_json::json!({
                    "kind": "read",
                    "drive": package_name,
                }))
                .unwrap(),
            });
            requested_caps.insert(Capability {
                issuer: Address {
                    node: our_name.into(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::to_string(&serde_json::json!({
                    "kind": "write",
                    "drive": package_name,
                }))
                .unwrap(),
            });

            let public_process = entry.public;

            // queue the granted capabilities
            // for process_name in &entry.public {
            //     if process_name == "all" {
            //         public_process = true;
            //         continue;
            //     }
            //     let process_id = ProcessId::from_str(process_name).unwrap();
            //     caps_to_grant.push((
            //         process_id.clone(),
            //         Capability {
            //             issuer: Address {
            //                 node: our_name.to_string(),
            //                 process: ProcessId::from_str(&our_process_id).unwrap(),
            //             },
            //             params: "\"messaging\"".into(),
            //         },
            //     ));
            // }

            // save in process map
            let file = FileIdentifier::new_uuid();
            manifest.write(&file, &wasm_bytes).await.unwrap();
            let wasm_bytes_handle = file.to_uuid().unwrap();

            process_map.insert(
                ProcessId::new(Some(&entry.process_name), package_name, package_publisher),
                PersistedProcess {
                    wasm_bytes_handle,
                    on_panic: entry.on_panic,
                    capabilities: requested_caps,
                    public: public_process,
                },
            );
        }
    }

    // grant queued capabilities from all packages
    for (to, cap) in caps_to_grant {
        let Some(proc) = process_map.get_mut(&to) else {
            continue;
        };
        proc.capabilities.insert(cap);
    }

    // save kernel process state. FsAction::SetState(kernel)
    let serialized_process_map =
        bincode::serialize(&process_map).expect("state map serialization error!");
    let process_map_hash: [u8; 32] = hash_bytes(&serialized_process_map);

    if manifest.get_by_hash(&process_map_hash).await.is_none() {
        let _ = manifest
            .write(&kernel_process_id, &serialized_process_map)
            .await;
    }
    Ok(vfs_messages)
}

/// go into /target folder and get all .zip package files
async fn get_zipped_packages() -> Vec<(String, zip::ZipArchive<std::io::Cursor<Vec<u8>>>)> {
    println!("fs: reading distro packages...\r");
    let target_path = std::path::Path::new("target");

    let mut packages = Vec::new();

    if let Ok(mut entries) = fs::read_dir(target_path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().ends_with(".zip") {
                let package_name = entry
                    .file_name()
                    .to_string_lossy()
                    .trim_end_matches(".zip")
                    .to_string();
                if let Ok(bytes) = fs::read(entry.path()).await {
                    if let Ok(zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
                        // add to list of packages
                        println!("fs: found package: {}\r", package_name);
                        packages.push((package_name, zip));
                    }
                }
            }
        }
    }

    return packages;
}

pub async fn fs_sender(
    our_name: String,
    manifest: Manifest,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_in_fs: MessageReceiver,
    mut recv_kill: Receiver<()>,
    send_kill_confirm: Sender<()>,
) -> Result<()> {
    //  process queues for consistency
    //  todo: use file_drive for moar concurrency!
    let process_queues = Arc::new(Mutex::new(
        HashMap::<ProcessId, VecDeque<KernelMessage>>::new(),
    ));

    //  interval for deleting/(flushing), don't want to run immediately upon bootup.
    //  -> separate wal_flush and cold_flush
    let mut interval = interval(Duration::from_secs(manifest.flush_cold_freq as u64));
    let mut first_open = true;

    //  into main loop
    loop {
        tokio::select! {
            Some(kernel_message) = recv_in_fs.recv() => {
                if our_name != kernel_message.source.node {
                    println!(
                        "fs: request must come from our_name={}, got: {}",
                        our_name, &kernel_message,
                    );
                    continue;
                }

                //  internal structures have Arc::clone setup.
                let manifest_clone = manifest.clone();

                let our_name = our_name.clone();
                let source = kernel_message.source.clone();
                let send_to_loop = send_to_loop.clone();
                let send_to_terminal = send_to_terminal.clone();

                let mut process_lock = process_queues.lock().await;

                if let Some(queue) = process_lock.get_mut(&source.process) {
                    queue.push_back(kernel_message.clone());
                } else {
                    let mut new_queue = VecDeque::new();
                    new_queue.push_back(kernel_message.clone());
                    process_lock.insert(source.process.clone(), new_queue);

                    // clone Arc for thread
                    let process_lock_clone = process_queues.clone();

                    tokio::spawn(async move {
                        let mut process_lock = process_lock_clone.lock().await;

                        while let Some(km) = process_lock.get_mut(&source.process).and_then(|q| q.pop_front()) {
                            if let Err(e) = handle_request(
                                our_name.clone(),
                                km.clone(),
                                manifest_clone.clone(),
                                send_to_loop.clone(),
                                send_to_terminal.clone(),
                            )
                            .await
                            {
                                let _ = send_to_loop
                                    .send(make_error_message(our_name.clone(), &km, e))
                                    .await;
                            }
                        }
                        // Remove the process entry if no more tasks are left
                        if let Some(queue) = process_lock.get(&source.process) {
                            if queue.is_empty() {
                                process_lock.remove(&source.process);
                            }
                        }
                    });
                }
            }
            _ = interval.tick() => {
                if !first_open {
                    let manifest_clone = manifest.clone();

                    tokio::spawn(async move {
                        let _ = manifest_clone.flush_to_cold().await;
                        //  let _ = manifest_clone.cleanup().await;
                    });
                }
                first_open = false;
            }
            _ = &mut recv_kill => {
                let manifest_clone = manifest.clone();
                let _ = manifest_clone.flush_to_wal_main().await;

                let _ = send_kill_confirm.send(());
                return Ok(());
            }
        }
    }
}

async fn handle_request(
    our_name: String,
    kernel_message: KernelMessage,
    manifest: Manifest,
    send_to_loop: MessageSender,
    _send_to_terminal: PrintSender,
) -> Result<(), FsError> {
    let KernelMessage {
        id,
        source,
        rsvp,
        message,
        payload,
        ..
    } = kernel_message;
    let Message::Request(Request {
        expects_response,
        ipc: Some(json_string),
        metadata, // for kernel
        ..
    }) = message
    else {
        return Err(FsError::NoJson);
    };

    let action: FsAction = match serde_json::from_str(&json_string) {
        Ok(r) => r,
        Err(e) => {
            return Err(FsError::BadJson {
                json: json_string.into(),
                error: format!("parse failed: {:?}", e),
            })
        }
    };

    // println!("got action! {:?}", action);

    let (ipc, bytes) = match action {
        FsAction::Write => {
            let Some(ref payload) = payload else {
                return Err(FsError::BadBytes {
                    action: "Write".into(),
                });
            };
            // println!("fs: got write from {:?} with len {:?}", source.process, &payload.bytes.len());

            let file_uuid = FileIdentifier::new_uuid();
            match manifest.write(&file_uuid, &payload.bytes).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(FsError::WriteFailed {
                        file: file_uuid.to_uuid().unwrap_or_default(),
                        error: e.to_string(),
                    })
                }
            }

            (FsResponse::Write(file_uuid.to_uuid().unwrap()), None)
        }
        FsAction::WriteOffset((file_uuid, offset)) => {
            let Some(ref payload) = payload else {
                return Err(FsError::BadBytes {
                    action: "Write".into(),
                });
            };

            let file_uuid = FileIdentifier::UUID(file_uuid);

            match manifest.write_at(&file_uuid, offset, &payload.bytes).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(FsError::WriteFailed {
                        file: file_uuid.to_uuid().unwrap_or_default(),
                        error: format!("write_offset error: {}", e),
                    })
                }
            }

            (FsResponse::Write(file_uuid.to_uuid().unwrap()), None)
        }
        FsAction::Read(file_uuid) => {
            let file = FileIdentifier::UUID(file_uuid);

            match manifest.read(&file, None, None).await {
                Err(e) => {
                    return Err(FsError::ReadFailed {
                        file: file.to_uuid().unwrap_or_default(),
                        error: e.to_string(),
                    })
                }
                Ok(bytes) => (FsResponse::Read(file_uuid), Some(bytes)),
            }
        }
        FsAction::ReadChunk(req) => {
            let file = FileIdentifier::UUID(req.file);

            match manifest
                .read(&file, Some(req.start), Some(req.length))
                .await
            {
                Err(e) => {
                    return Err(FsError::ReadFailed {
                        file: file.to_uuid().unwrap_or_default(),
                        error: e.to_string(),
                    })
                }
                Ok(bytes) => (FsResponse::Read(req.file), Some(bytes)),
            }
        }
        FsAction::Replace(old_file_uuid) => {
            let Some(ref payload) = payload else {
                return Err(FsError::BadBytes {
                    action: "Write".into(),
                });
            };

            let file = FileIdentifier::UUID(old_file_uuid);
            match manifest.write(&file, &payload.bytes).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(FsError::WriteFailed {
                        file: old_file_uuid,
                        error: format!("replace error: {}", e),
                    })
                }
            }

            (FsResponse::Write(old_file_uuid), None)
        }
        FsAction::Delete(del) => {
            let file = FileIdentifier::UUID(del);
            manifest.delete(&file).await?;

            (FsResponse::Delete(del), None)
        }
        FsAction::Append(maybe_file_uuid) => {
            let Some(ref payload) = payload else {
                return Err(FsError::BadBytes {
                    action: "Append".into(),
                });
            };

            let file_uuid = match maybe_file_uuid {
                Some(uuid) => FileIdentifier::UUID(uuid),
                None => FileIdentifier::new_uuid(),
            };

            match manifest.append(&file_uuid, &payload.bytes).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(FsError::WriteFailed {
                        file: file_uuid.to_uuid().unwrap_or_default(),
                        error: format!("append error: {}", e),
                    })
                }
            };
            // note expecting file_uuid here, if we want process state to access append, we would change this.
            (FsResponse::Append(file_uuid.to_uuid().unwrap()), None)
        }
        FsAction::Length(file_uuid) => {
            let file = FileIdentifier::UUID(file_uuid);
            let length = manifest.get_length(&file).await;
            match length {
                Some(len) => (FsResponse::Length(len), None),
                None => {
                    return Err(FsError::LengthError {
                        error: format!("file not found: {:?}", file_uuid),
                    })
                }
            }
        }
        FsAction::SetLength((file_uuid, length)) => {
            let file = FileIdentifier::UUID(file_uuid);
            manifest.set_length(&file, length).await?;

            // doublecheck if this is the type of return statement we want.
            (FsResponse::Length(length), None)
        }
        //  process state handlers
        FsAction::SetState(process_id) => {
            let Some(ref payload) = payload else {
                return Err(FsError::BadBytes {
                    action: "SetState".into(),
                });
            };

            // println!("setting state for process {:?} with len {:?}", process_id, &payload.bytes.len());
            let file = FileIdentifier::Process(process_id);
            match manifest.write(&file, &payload.bytes).await {
                Ok(_) => (),
                Err(e) => {
                    return Err(FsError::WriteFailed {
                        file: file.to_uuid().unwrap_or_default(),
                        error: format!("SetState error: {}", e),
                    })
                }
            };

            (FsResponse::SetState, None)
        }
        FsAction::DeleteState(process_id) => {
            let file = FileIdentifier::Process(process_id);
            manifest.delete(&file).await?;

            (FsResponse::Delete(0), None)
        }
        FsAction::GetState(process_id) => {
            let file = FileIdentifier::Process(process_id);

            match manifest.read(&file, None, None).await {
                Err(e) => return Err(e),
                Ok(bytes) => (FsResponse::GetState, Some(bytes)),
            }
        }
    };

    if expects_response.is_some() {
        let response = KernelMessage {
            id: id.clone(),
            source: Address {
                node: our_name.clone(),
                process: FILESYSTEM_PROCESS_ID.clone(),
            },
            target: match rsvp {
                None => source,
                Some(rsvp) => rsvp,
            },
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc: Some(
                        serde_json::to_string::<Result<FsResponse, FsError>>(&Ok(ipc)).unwrap(),
                    ),
                    metadata, // for kernel
                },
                None,
            )),
            payload: match bytes {
                Some(bytes) => Some(Payload { mime: None, bytes }),
                None => None,
            },
            signed_capabilities: None,
        };

        let _ = send_to_loop.send(response).await;
    }

    Ok(())
}

/// HELPERS

pub fn hash_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    let chunk_size: usize = 1024 * 256;

    for chunk in bytes.chunks(chunk_size) {
        let chunk_hash: [u8; 32] = blake3::hash(chunk).into();
        hasher.update(&chunk_hash);
    }
    hasher.finalize().into()
}

fn make_error_message(our_name: String, km: &KernelMessage, error: FsError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: FILESYSTEM_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: Some(
                    serde_json::to_string::<Result<FsResponse, FsError>>(&Err(error)).unwrap(),
                ),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
