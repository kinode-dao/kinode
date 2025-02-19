use lib::types::core::{
    check_process_id_hypermap_safe, Address, Capability, Erc721Metadata, KernelMessage, LazyLoadBlob,
    Message, MessageReceiver, MessageSender, NetworkErrorSender, OnExit, PackageManifestEntry,
    PersistedProcess, PrintSender, Printout, ProcessId, ProcessMap, Request, Response,
    ReverseCapIndex, StateAction, StateError, StateResponse, KERNEL_PROCESS_ID, STATE_PROCESS_ID,
    VFS_PROCESS_ID,
};
use ring::signature;
use rocksdb::{checkpoint::Checkpoint, Options, DB};
use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    path::PathBuf,
    sync::Arc,
};
use tokio::{fs, io::AsyncWriteExt, sync::Mutex};

static PACKAGES_ZIP: &[u8] = include_bytes!("../../target/packages.zip");
const FILE_TO_METADATA: &str = "file_to_metadata.json";

pub async fn load_state(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_string: String,
    runtime_extensions: Vec<(ProcessId, MessageSender, Option<NetworkErrorSender>, bool)>,
) -> Result<(ProcessMap, DB, ReverseCapIndex), StateError> {
    let home_directory_path = std::fs::canonicalize(&home_directory_string)?;
    let state_path = home_directory_path.join("kernel");
    if let Err(e) = fs::create_dir_all(&state_path).await {
        panic!("failed creating kernel state dir! {e:?}");
    }
    // use String to not upset rocksdb:
    //  * on Unix, works as expected
    //  * on Windows, would normally use std::path to be cross-platform,
    //    but here rocksdb appends a `/LOG` which breaks the path
    let state_path = format!("{home_directory_string}/kernel");

    let mut opts = Options::default();
    opts.create_if_missing(true);
    let db = DB::open_default(state_path).unwrap();
    let mut process_map: ProcessMap = HashMap::new();
    let mut reverse_cap_index: ReverseCapIndex = HashMap::new();

    let kernel_id_vec = process_to_vec(KERNEL_PROCESS_ID.clone());
    match db.get(&kernel_id_vec) {
        Ok(Some(value)) => {
            process_map = bincode::deserialize::<ProcessMap>(&value)
                .expect("failed to deserialize kernel process map");
            // if our networking key changed, we need to re-sign all local caps
            process_map.iter_mut().for_each(|(_id, process)| {
                process.capabilities.iter_mut().for_each(|(cap, sig)| {
                    if cap.issuer.node == our_name {
                        *sig = keypair
                            .sign(&rmp_serde::to_vec(&cap).unwrap())
                            .as_ref()
                            .to_vec();
                    }
                })
            });
        }
        Ok(None) => {
            db.put(&kernel_id_vec, bincode::serialize(&process_map).unwrap())
                .unwrap();
        }
        Err(e) => {
            panic!("failed to load kernel state from db: {e:?}");
        }
    }

    let processes = process_map.keys().cloned().collect::<Vec<_>>();
    for process in processes {
        if check_process_id_hypermap_safe(&process).is_err() {
            println!("bootstrap: removing non-Hypermap-safe process {process}\n(all process IDs must contain only a-z, 0-9, `-`, and `.`s in the publisher)\r");
            process_map.remove(&process);
        }
    }

    // bootstrap the distro processes into the node
    bootstrap(
        &our_name,
        keypair,
        home_directory_path,
        runtime_extensions,
        &mut process_map,
        &mut reverse_cap_index,
    )
    .await
    .expect("bootstrapping filesystem failed!");

    Ok((process_map, db, reverse_cap_index))
}

pub async fn state_sender(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_state: MessageReceiver,
    db: DB,
    home_directory_path: PathBuf,
) -> Result<(), anyhow::Error> {
    let db = Arc::new(db);
    let home_directory_path = Arc::new(home_directory_path);

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> = HashMap::new();

    while let Some(km) = recv_state.recv().await {
        if *our_node != km.source.node {
            Printout::new(
                1,
                STATE_PROCESS_ID.clone(),
                format!(
                    "state: got request from {}, but requests must come from our node {our_node}",
                    km.source.node
                ),
            )
            .send(&send_to_terminal)
            .await;
            continue;
        }

        let queue = process_queues
            .get(&km.source.process)
            .cloned()
            .unwrap_or_else(|| Arc::new(Mutex::new(VecDeque::new())));

        {
            let mut queue_lock = queue.lock().await;
            queue_lock.push_back(km);
        }

        let our_node = our_node.clone();
        let db_clone = db.clone();
        let send_to_loop = send_to_loop.clone();
        let home_directory_path = home_directory_path.clone();

        tokio::spawn(async move {
            let mut queue_lock = queue.lock().await;
            if let Some(km) = queue_lock.pop_front() {
                let (km_id, km_rsvp) =
                    (km.id.clone(), km.rsvp.clone().unwrap_or(km.source.clone()));

                if let Err(e) =
                    handle_request(&our_node, km, db_clone, &send_to_loop, &home_directory_path)
                        .await
                {
                    KernelMessage::builder()
                        .id(km_id)
                        .source((our_node.as_str(), STATE_PROCESS_ID.clone()))
                        .target(km_rsvp)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: serde_json::to_vec(&StateResponse::Err(e)).unwrap(),
                                metadata: None,
                                capabilities: vec![],
                            },
                            None,
                        )))
                        .build()
                        .unwrap()
                        .send(&send_to_loop)
                        .await;
                }
            }
        });
    }
    Ok(())
}

async fn handle_request(
    our_node: &str,
    kernel_message: KernelMessage,
    db: Arc<DB>,
    send_to_loop: &MessageSender,
    home_directory_path: &PathBuf,
) -> Result<(), StateError> {
    let KernelMessage {
        id,
        source,
        rsvp,
        message,
        lazy_load_blob: blob,
        ..
    } = kernel_message;
    let Message::Request(Request {
        expects_response,
        body,
        metadata, // for kernel
        ..
    }) = message
    else {
        return Err(StateError::BadRequest {
            error: "not a request".into(),
        });
    };

    let action: StateAction = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return Err(StateError::BadJson {
                error: format!("parse into StateAction failed: {e:?}"),
            })
        }
    };

    let (body, bytes) = match action {
        StateAction::SetState(process_id) => {
            let key = process_to_vec(process_id);

            let Some(ref blob) = blob else {
                return Err(StateError::BadBytes {
                    action: "SetState".into(),
                });
            };

            db.put(key, &blob.bytes)
                .map_err(|e| StateError::RocksDBError {
                    action: "SetState".into(),
                    error: e.to_string(),
                })?;

            (serde_json::to_vec(&StateResponse::SetState).unwrap(), None)
        }
        StateAction::GetState(process_id) => {
            let key = process_to_vec(process_id.clone());
            match db.get(key) {
                Ok(Some(value)) => (
                    serde_json::to_vec(&StateResponse::GetState).unwrap(),
                    Some(value),
                ),
                Ok(None) => {
                    return Err(StateError::NotFound {
                        process_id: process_id.clone(),
                    });
                }
                Err(e) => {
                    return Err(StateError::RocksDBError {
                        action: "GetState".into(),
                        error: e.to_string(),
                    });
                }
            }
        }
        StateAction::DeleteState(process_id) => {
            let key = process_to_vec(process_id);
            match db.delete(key) {
                Ok(_) => (
                    serde_json::to_vec(&StateResponse::DeleteState).unwrap(),
                    None,
                ),
                Err(e) => {
                    return Err(StateError::RocksDBError {
                        action: "DeleteState".into(),
                        error: e.to_string(),
                    });
                }
            }
        }
        StateAction::Backup => {
            let checkpoint_dir = home_directory_path.join("kernel").join("backup");
            if checkpoint_dir.exists() {
                fs::remove_dir_all(&checkpoint_dir).await?;
            }
            let checkpoint = Checkpoint::new(&db).map_err(|e| StateError::RocksDBError {
                action: "BackupCheckpointNew".into(),
                error: e.to_string(),
            })?;

            checkpoint.create_checkpoint(&checkpoint_dir).map_err(|e| {
                StateError::RocksDBError {
                    action: "BackupCheckpointCreate".into(),
                    error: e.to_string(),
                }
            })?;

            (serde_json::to_vec(&StateResponse::Backup).unwrap(), None)
        }
    };

    if let Some(target) = rsvp.or_else(|| expects_response.map(|_| source)) {
        KernelMessage::builder()
            .id(id)
            .source((our_node, STATE_PROCESS_ID.clone()))
            .target(target)
            .message(Message::Response((
                Response {
                    inherit: false,
                    body,
                    metadata,
                    capabilities: vec![],
                },
                None,
            )))
            .lazy_load_blob(bytes.map(|bytes| LazyLoadBlob {
                mime: Some("application/octet-stream".into()),
                bytes,
            }))
            .build()
            .unwrap()
            .send(send_to_loop)
            .await;
    };

    Ok(())
}

/// function run only upon fresh boot.
///
/// for each included package.zip file, extracts the contents,
/// sends the contents to VFS, and reads the manifest.json.
///
/// the manifest.json contains instructions for which processes to boot and what
/// capabilities to give them. since we are inside runtime, can spawn those out of
/// thin air.
async fn bootstrap(
    our_name: &str,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: PathBuf,
    runtime_extensions: Vec<(ProcessId, MessageSender, Option<NetworkErrorSender>, bool)>,
    process_map: &mut ProcessMap,
    reverse_cap_index: &mut ReverseCapIndex,
) -> anyhow::Result<()> {
    let mut runtime_caps: HashMap<Capability, Vec<u8>> = HashMap::new();
    // kernel is a special case
    let k_cap = Capability {
        issuer: Address {
            node: our_name.to_string(),
            process: ProcessId::new(Some("kernel"), "distro", "sys"),
        },
        params: "\"messaging\"".into(),
    };
    runtime_caps.insert(k_cap.clone(), sign_cap(k_cap, keypair.clone()));
    // net is a special case
    let n_cap = Capability {
        issuer: Address {
            node: our_name.to_string(),
            process: ProcessId::new(Some("net"), "distro", "sys"),
        },
        params: "\"messaging\"".into(),
    };
    runtime_caps.insert(n_cap.clone(), sign_cap(n_cap, keypair.clone()));
    for runtime_module in runtime_extensions.clone() {
        let m_cap = Capability {
            issuer: Address {
                node: our_name.to_string(),
                process: runtime_module.0,
            },
            params: "\"messaging\"".into(),
        };
        runtime_caps.insert(m_cap.clone(), sign_cap(m_cap, keypair.clone()));
    }
    // give all runtime processes the ability to send messages across the network
    let net_cap = Capability {
        issuer: Address {
            node: our_name.to_string(),
            process: KERNEL_PROCESS_ID.clone(),
        },
        params: "\"network\"".into(),
    };
    runtime_caps.insert(net_cap.clone(), sign_cap(net_cap, keypair.clone()));

    // finally, save runtime modules in state map as well, somewhat fakely
    // special cases for kernel and net
    let current_kernel = process_map
        .entry(ProcessId::new(Some("kernel"), "distro", "sys"))
        .or_insert(PersistedProcess {
            wasm_bytes_handle: "".into(),
            wit_version: Some(crate::kernel::LATEST_WIT_VERSION),
            on_exit: OnExit::Restart,
            capabilities: runtime_caps.clone(),
            public: false,
        });
    current_kernel.capabilities.extend(runtime_caps.clone());
    let current_net = process_map
        .entry(ProcessId::new(Some("net"), "distro", "sys"))
        .or_insert(PersistedProcess {
            wasm_bytes_handle: "".into(),
            wit_version: Some(crate::kernel::LATEST_WIT_VERSION),
            on_exit: OnExit::Restart,
            capabilities: runtime_caps.clone(),
            public: false,
        });
    current_net.capabilities.extend(runtime_caps.clone());
    for runtime_module in runtime_extensions {
        let current = process_map
            .entry(runtime_module.0)
            .or_insert(PersistedProcess {
                wasm_bytes_handle: "".into(),
                wit_version: Some(crate::kernel::LATEST_WIT_VERSION),
                on_exit: OnExit::Restart,
                capabilities: runtime_caps.clone(),
                public: runtime_module.3,
            });
        current.capabilities.extend(runtime_caps.clone());
    }

    let packages = get_zipped_packages();

    for (package_metadata, mut package) in packages.clone() {
        let package_name = package_metadata.properties.package_name.as_str();
        // special case tester: only load it in if in simulation mode
        #[cfg(not(feature = "simulation-mode"))]
        if package_name == "tester" {
            continue;
        }

        let package_publisher = package_metadata.properties.publisher.as_str();

        // create a new package in VFS
        #[cfg(unix)]
        let our_drive_name = [package_name, package_publisher].join(":");
        #[cfg(target_os = "windows")]
        let our_drive_name = [package_name, package_publisher].join("_");
        let pkg_path = home_directory_path
            .join("vfs")
            .join(&our_drive_name)
            .join("pkg");

        // delete anything currently residing in the pkg folder
        if pkg_path.exists() {
            fs::remove_dir_all(&pkg_path).await?;
        }
        fs::create_dir_all(&pkg_path)
            .await
            .expect("bootstrap vfs dir pkg creation failed!");

        let drive_path = format!("/{}/pkg", [package_name, package_publisher].join(":"));

        // save the zip itself inside pkg folder, for sharing with others
        let mut zip_file =
            fs::File::create(pkg_path.join(format!("{}.zip", &our_drive_name))).await?;
        let package_zip_bytes = package.clone().into_inner().into_inner();
        zip_file.write_all(&package_zip_bytes).await?;

        // for each file in package.zip, write to vfs folder
        for i in 0..package.len() {
            let mut file = match package.by_index(i) {
                Ok(f) => f,
                Err(e) => {
                    println!("Error accessing file by index: {}", e);
                    continue;
                }
            };

            let file_path = match file.enclosed_name() {
                Some(path) => path.to_owned(),
                None => {
                    println!("Error getting the file name from the package");
                    continue;
                }
            };

            let file_path_str = file_path.to_string_lossy().to_string();
            let full_path = pkg_path.join(&file_path_str);

            if file.is_dir() {
                // It's a directory, create it
                if let Err(e) = fs::create_dir_all(&full_path).await {
                    println!("Failed to create directory {}: {}", full_path.display(), e);
                }
            } else if file.is_file() {
                // It's a file, ensure the parent directory exists and write the file
                if let Some(parent) = full_path.parent() {
                    if let Err(e) = fs::create_dir_all(parent).await {
                        println!("Failed to create parent directory: {}", e);
                        continue;
                    }
                }

                let mut file_content = Vec::new();
                if let Err(e) = file.read_to_end(&mut file_content) {
                    println!("Error reading file contents: {}", e);
                    continue;
                }

                // Write the file content
                if let Err(e) = fs::write(&full_path, file_content).await {
                    println!("Failed to write file {}: {}", full_path.display(), e);
                }
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
            let mut requested_caps = HashMap::new();
            let our_process_id = format!(
                "{}:{}:{}",
                entry.process_name, package_name, package_publisher
            );
            entry
                .request_capabilities
                .push(serde_json::Value::String(our_process_id.clone()));
            for value in entry.request_capabilities {
                let requested_cap = match value {
                    serde_json::Value::String(process_name) => Capability {
                        issuer: Address {
                            node: our_name.to_string(),
                            process: process_name.parse().unwrap(),
                        },
                        params: "\"messaging\"".into(),
                    },
                    serde_json::Value::Object(map) => {
                        if let Some(process_name) = map.get("process") {
                            if let Some(params) = map.get("params") {
                                Capability {
                                    issuer: Address {
                                        node: our_name.to_string(),
                                        process: process_name.as_str().unwrap().parse().unwrap(),
                                    },
                                    params: params.to_string(),
                                }
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }
                    _ => {
                        // other json types
                        continue;
                    }
                };
                requested_caps.insert(
                    requested_cap.clone(),
                    sign_cap(requested_cap, keypair.clone()),
                );
            }

            if entry.request_networking {
                let net_cap = Capability {
                    issuer: Address {
                        node: our_name.to_string(),
                        process: KERNEL_PROCESS_ID.clone(),
                    },
                    params: "\"network\"".into(),
                };
                requested_caps.insert(net_cap.clone(), sign_cap(net_cap, keypair.clone()));
            }

            // give access to package_name vfs
            let read_cap = Capability {
                issuer: Address {
                    node: our_name.into(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::json!({
                    "kind": "read",
                    "drive": drive_path,
                })
                .to_string(),
            };
            requested_caps.insert(read_cap.clone(), sign_cap(read_cap, keypair.clone()));
            let write_cap = Capability {
                issuer: Address {
                    node: our_name.into(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::json!({
                    "kind": "write",
                    "drive": drive_path,
                })
                .to_string(),
            };
            requested_caps.insert(write_cap.clone(), sign_cap(write_cap, keypair.clone()));

            let public_process = entry.public;

            let wasm_bytes_handle = format!("{}/{}", &drive_path, &file_path);

            match process_map.entry(ProcessId::new(
                Some(&entry.process_name),
                package_name,
                package_publisher,
            )) {
                std::collections::hash_map::Entry::Occupied(p) => {
                    let p = p.into_mut();
                    p.wasm_bytes_handle = wasm_bytes_handle.clone();
                    p.wit_version = package_metadata.properties.wit_version;
                    p.on_exit = entry.on_exit;
                    p.capabilities.extend(requested_caps);
                    p.public = public_process;
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(PersistedProcess {
                        wasm_bytes_handle: wasm_bytes_handle.clone(),
                        wit_version: package_metadata.properties.wit_version,
                        on_exit: entry.on_exit,
                        capabilities: requested_caps,
                        public: public_process,
                    });
                }
            }
        }
    }
    // second loop: go and grant_capabilities to processes
    // can't do this in first loop because we need to have all processes in the map first
    for (package_metadata, mut package) in packages {
        let package_name = package_metadata.properties.package_name.as_str();
        // special case tester: only load it in if in simulation mode
        #[cfg(not(feature = "simulation-mode"))]
        if package_name == "tester" {
            continue;
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

        let package_publisher = package_metadata.properties.publisher.as_str();

        for entry in package_manifest {
            let our_process_id = format!(
                "{}:{}:{}",
                entry.process_name, package_name, package_publisher
            );

            // grant capabilities to other initially spawned processes, distro
            for value in entry.grant_capabilities {
                match value {
                    serde_json::Value::String(process_name) => {
                        if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                            if let Some(process) = process_map.get_mut(&parsed_process_id) {
                                let cap = Capability {
                                    issuer: Address {
                                        node: our_name.to_string(),
                                        process: our_process_id.parse().unwrap(),
                                    },
                                    params: "\"messaging\"".into(),
                                };
                                process
                                    .capabilities
                                    .insert(cap.clone(), sign_cap(cap.clone(), keypair.clone()));
                                reverse_cap_index
                                    .entry(cap.clone().issuer.process)
                                    .or_insert_with(HashMap::new)
                                    .entry(our_process_id.parse().unwrap())
                                    .or_insert_with(Vec::new)
                                    .push(cap);
                            }
                        }
                    }
                    serde_json::Value::Object(map) => {
                        if let Some(process_name) = map.get("process") {
                            if let Ok(parsed_process_id) =
                                process_name.as_str().unwrap().parse::<ProcessId>()
                            {
                                if let Some(params) = map.get("params") {
                                    if let Some(process) = process_map.get_mut(&parsed_process_id) {
                                        let cap = Capability {
                                            issuer: Address {
                                                node: our_name.to_string(),
                                                process: our_process_id.parse().unwrap(),
                                            },
                                            params: params.to_string(),
                                        };
                                        process.capabilities.insert(
                                            cap.clone(),
                                            sign_cap(cap.clone(), keypair.clone()),
                                        );
                                        reverse_cap_index
                                            .entry(cap.clone().issuer.process)
                                            .or_insert_with(HashMap::new)
                                            .entry(our_process_id.parse().unwrap())
                                            .or_insert_with(Vec::new)
                                            .push(cap);
                                    }
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
    }
    Ok(())
}

fn sign_cap(cap: Capability, keypair: Arc<signature::Ed25519KeyPair>) -> Vec<u8> {
    keypair
        .sign(&rmp_serde::to_vec(&cap).unwrap())
        .as_ref()
        .to_vec()
}

/// read in `include!()`ed .zip package files
fn get_zipped_packages() -> Vec<(Erc721Metadata, zip::ZipArchive<std::io::Cursor<Vec<u8>>>)> {
    let mut packages = Vec::new();

    let mut packages_zip = zip::ZipArchive::new(std::io::Cursor::new(PACKAGES_ZIP)).unwrap();
    let mut file_to_metadata = vec![];
    packages_zip
        .by_name(FILE_TO_METADATA)
        .unwrap()
        .read_to_end(&mut file_to_metadata)
        .unwrap();
    let file_to_metadata: HashMap<String, Erc721Metadata> =
        serde_json::from_slice(&file_to_metadata).unwrap();

    for (file_name, metadata) in file_to_metadata {
        let mut zip_bytes = vec![];
        packages_zip
            .by_name(&file_name)
            .unwrap()
            .read_to_end(&mut zip_bytes)
            .unwrap();
        let zip_archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
        packages.push((metadata, zip_archive));
    }

    packages
}

fn process_to_vec(process: ProcessId) -> Vec<u8> {
    process.to_string().as_bytes().to_vec()
}
