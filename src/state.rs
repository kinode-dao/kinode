use crate::filesystem::manifest::{FileIdentifier, Manifest};
use crate::types::*;
use anyhow::Result;
use rocksdb::backup::{BackupEngine, BackupEngineOptions};
use rocksdb::checkpoint::Checkpoint;
use rocksdb::{ColumnFamilyDescriptor, Env, Options, DB};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

pub async fn load_state(
    our_name: String,
    home_directory_path: String,
    runtime_extensions: Vec<(ProcessId, MessageSender, bool)>,
) -> Result<(ProcessMap, DB, Vec<KernelMessage>), FsError> {
    let state_path = format!("{}/kernel", &home_directory_path);

    if let Err(e) = fs::create_dir_all(&state_path).await {
        panic!("failed creating kernel state dir! {:?}", e);
    }

    // more granular kernel_state in column families

    // let mut options = Option::default().unwrap();
    // options.create_if_missing(true);
    //let db = DB::open_default(&state_directory_path_str).unwrap();
    let mut opts = Options::default();
    opts.create_if_missing(true);
    // let cf_name = "kernel_state";
    // let cf_descriptor = ColumnFamilyDescriptor::new(cf_name, Options::default());
    let mut db = DB::open_default(state_path).unwrap();
    let mut process_map: ProcessMap = HashMap::new();

    // kernel hash?

    let vfs_messages = match db.get(KERNEL_PROCESS_ID.to_hash()) {
        Ok(Some(value)) => {
            process_map = bincode::deserialize(&value).expect("state map deserialization error!");
            vec![]
        }
        Ok(None) => bootstrap(&our_name, runtime_extensions, &mut process_map, &mut db)
            .await
            .expect("fresh bootstrap failed!"),
        Err(e) => panic!("operational problem encountered: {}", e),
    };
    println!("booted process map: {:?}", process_map);
    Ok((process_map, db, vfs_messages))
}

pub async fn state_sender(
    our_name: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_state: MessageReceiver,
    db: DB,
    home_directory_path: String,
    // mut recv_kill: Receiver<()>,
    // send_kill_confirm: Sender<()>,
) -> Result<(), anyhow::Error> {
    let db = Arc::new(db);
    //  into main loop

    loop {
        tokio::select! {
            Some(km) = recv_state.recv() => {
                if our_name != km.source.node {
                    println!(
                        "fs: request must come from our_name={}, got: {}",
                        our_name, &km,
                    );
                    continue;
                }
                let db_clone = db.clone();
                let send_to_loop = send_to_loop.clone();
                let send_to_terminal = send_to_terminal.clone();
                let our_name = our_name.clone();
                let home_directory_path = home_directory_path.clone();

                tokio::spawn(async move {

                    if let Err(e) = handle_request(
                            our_name.clone(),
                            km.clone(),
                            db_clone,
                            send_to_loop.clone(),
                            send_to_terminal,
                            home_directory_path,
                        )
                        .await
                        {
                            let _ = send_to_loop
                                .send(make_error_message(our_name.clone(), &km, e))
                                .await;
                        }
                });

                // tokio async move?
                //  tokio spawn_blocking or block_inplace here?
                //  internal structures have Arc::clone setup.
            }
        }
    }
}

async fn handle_request(
    our_name: String,
    kernel_message: KernelMessage,
    db: Arc<DB>,
    send_to_loop: MessageSender,
    _send_to_terminal: PrintSender,
    home_directory_path: String,
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
        ipc,
        metadata, // for kernel
        ..
    }) = message
    else {
        return Err(FsError::NoJson);
    };

    let action: StateAction = match serde_json::from_slice(&ipc) {
        Ok(r) => r,
        Err(e) => {
            return Err(FsError::BadJson {
                json: String::from_utf8(ipc).unwrap_or_default(),
                error: format!("parse failed: {:?}", e),
            })
        }
    };

    let (ipc, bytes) = match action {
        StateAction::Read(handle) => {
            // handle Read action
            let key = handle.to_le_bytes();
            match db.get(key) {
                Ok(Some(value)) => (StateResponse::Read(handle), Some(value)),
                Ok(None) => {
                    println!("nothing found");
                    return Err(FsError::NoJson);
                }
                Err(e) => {
                    println!("read rockdsb error: {:?}", e);
                    return Err(FsError::NoJson);
                }
            }
        }
        StateAction::SetState(process_id) => {
            let key = process_id.to_hash();
            let Some(ref payload) = payload else {
                return Err(FsError::BadBytes {
                    action: "SetState".into(),
                });
            };

            match db.put(key, &payload.bytes) {
                Ok(_) => {
                    println!("set state success");
                    (StateResponse::SetState, None)
                }
                Err(e) => {
                    println!("set state error: {:?}", e);
                    return Err(FsError::NoJson);
                }
            }
        }
        StateAction::GetState(process_id) => {
            let key = process_id.to_hash();
            match db.get(key) {
                Ok(Some(value)) => {
                    println!("found value");
                    (StateResponse::GetState, Some(value))
                }
                Ok(None) => {
                    println!("nothing found");
                    return Err(FsError::NoJson);
                }
                Err(e) => {
                    println!("get state error: {:?}", e);
                    return Err(FsError::NoJson);
                }
            }
        }
        StateAction::DeleteState(process_id) => {
            // handle DeleteState action
            println!("got deleteState");
            let key = process_id.to_hash();
            match db.delete(key) {
                Ok(_) => {
                    println!("delete state success");
                    (StateResponse::DeleteState, None)
                }
                Err(e) => {
                    println!("delete state error: {:?}", e);
                    return Err(FsError::NoJson);
                }
            }
        }
        StateAction::Backup => {
            // handle Backup action
            println!("got backup");
            let checkpoint_dir = format!("{}/kernel/checkpoint", &home_directory_path);

            if Path::new(&checkpoint_dir).exists() {
                let _ = fs::remove_dir_all(&checkpoint_dir).await;
            }
            let checkpoint = Checkpoint::new(&db).unwrap();
            checkpoint.create_checkpoint(&checkpoint_dir).unwrap();
            (StateResponse::Backup, None)
        }
    };

    if expects_response.is_some() {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_name.clone(),
                process: STATE_PROCESS_ID.clone(),
            },
            target: match rsvp {
                None => source,
                Some(rsvp) => rsvp,
            },
            rsvp: None,
            message: Message::Response((
                Response {
                    inherit: false,
                    ipc: serde_json::to_vec::<Result<StateResponse, FsError>>(&Ok(ipc)).unwrap(),
                    metadata, // for kernel
                },
                None,
            )),
            payload: bytes.map(|bytes| Payload { mime: None, bytes }),
            signed_capabilities: None,
        };

        let _ = send_to_loop.send(response).await;
    }

    Ok(())
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
    runtime_extensions: Vec<(ProcessId, MessageSender, bool)>,
    process_map: &mut ProcessMap,
    db: &mut DB,
) -> Result<Vec<KernelMessage>> {
    println!("bootstrapping node...\r");

    let mut runtime_caps: HashSet<Capability> = HashSet::new();
    // kernel is a special case
    runtime_caps.insert(Capability {
        issuer: Address {
            node: our_name.to_string(),
            process: ProcessId::from_str("kernel:sys:uqbar").unwrap(),
        },
        params: "\"messaging\"".into(),
    });
    // net is a special case
    runtime_caps.insert(Capability {
        issuer: Address {
            node: our_name.to_string(),
            process: ProcessId::from_str("net:sys:uqbar").unwrap(),
        },
        params: "\"messaging\"".into(),
    });
    for runtime_module in runtime_extensions.clone() {
        runtime_caps.insert(Capability {
            issuer: Address {
                node: our_name.to_string(),
                process: runtime_module.0,
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
    // special cases for kernel and net
    process_map
        .entry(ProcessId::from_str("kernel:sys:uqbar").unwrap())
        .or_insert(PersistedProcess {
            wasm_bytes_handle: 0,
            on_panic: OnPanic::Restart,
            capabilities: runtime_caps.clone(),
            public: false,
        });
    process_map
        .entry(ProcessId::from_str("net:sys:uqbar").unwrap())
        .or_insert(PersistedProcess {
            wasm_bytes_handle: 0,
            on_panic: OnPanic::Restart,
            capabilities: runtime_caps.clone(),
            public: false,
        });
    for runtime_module in runtime_extensions {
        process_map
            .entry(runtime_module.0)
            .or_insert(PersistedProcess {
                wasm_bytes_handle: 0,
                on_panic: OnPanic::Restart,
                capabilities: runtime_caps.clone(),
                public: runtime_module.2,
            });
    }

    let packages: Vec<(String, zip::ZipArchive<std::io::Cursor<Vec<u8>>>)> =
        get_zipped_packages().await;

    let mut vfs_messages = Vec::new();

    for (package_name, mut package) in packages {
        // special case tester: only load it in if in simulation mode
        if package_name == "tester" {
            #[cfg(not(feature = "simulation-mode"))]
            continue;
            #[cfg(feature = "simulation-mode")]
            {}
        }

        println!("fs: handling package {package_name}...\r");
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

        // create a new package in VFS
        let our_drive_name = [package_name, package_publisher].join(":");
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
                ipc: serde_json::to_vec::<VfsRequest>(&VfsRequest {
                    drive: our_drive_name.clone(),
                    action: VfsAction::New,
                })
                .unwrap(),
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
                if !file_path.starts_with('/') {
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
                        ipc: serde_json::to_vec::<VfsRequest>(&VfsRequest {
                            drive: our_drive_name.clone(),
                            action: VfsAction::Add {
                                full_path: file_path,
                                entry_type: AddEntryType::NewFile,
                            },
                        })
                        .unwrap(),
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
            let mut file_path = entry.process_wasm_path.to_string();
            if file_path.starts_with('/') {
                file_path = file_path[1..].to_string();
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
            entry.request_messaging = Some(entry.request_messaging.unwrap_or_default());
            if let Some(ref mut request_messaging) = entry.request_messaging {
                request_messaging.push(our_process_id.clone());
                for process_name in request_messaging {
                    requested_caps.insert(Capability {
                        issuer: Address {
                            node: our_name.to_string(),
                            process: ProcessId::from_str(process_name).unwrap(),
                        },
                        params: "\"messaging\"".into(),
                    });
                }
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
                    "drive": our_drive_name,
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
                    "drive": our_drive_name,
                }))
                .unwrap(),
            });

            let public_process = entry.public;

            // save in process map
            let file = FileIdentifier::new_uuid();
            let wasm_bytes_handle = file.to_uuid().unwrap();
            db.put(wasm_bytes_handle.to_le_bytes(), wasm_bytes).unwrap();

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

    // save kernel process state. FsAction::SetState(kernel)
    let serialized_process_map =
        bincode::serialize(&process_map).expect("state map serialization error!");

    db.put(KERNEL_PROCESS_ID.to_hash(), serialized_process_map)
        .unwrap();

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

    packages
}

fn make_error_message(our_name: String, km: &KernelMessage, error: FsError) -> KernelMessage {
    KernelMessage {
        id: km.id,
        source: Address {
            node: our_name.clone(),
            process: STATE_PROCESS_ID.clone(),
        },
        target: match &km.rsvp {
            None => km.source.clone(),
            Some(rsvp) => rsvp.clone(),
        },
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                ipc: serde_json::to_vec::<Result<StateResponse, FsError>>(&Err(error)).unwrap(),
                metadata: None,
            },
            None,
        )),
        payload: None,
        signed_capabilities: None,
    }
}
