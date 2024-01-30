use anyhow::Result;
use ring::signature;
use rocksdb::checkpoint::Checkpoint;
use rocksdb::{Options, DB};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::types::*;

include!("bootstrapped_processes.rs");

pub async fn load_state(
    our_name: String,
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
    runtime_extensions: Vec<(ProcessId, MessageSender, bool)>,
) -> Result<(ProcessMap, DB), StateError> {
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
    let db = DB::open_default(state_path).unwrap();
    let mut process_map: ProcessMap = HashMap::new();

    let kernel_id = process_to_vec(KERNEL_PROCESS_ID.clone());
    match db.get(&kernel_id) {
        Ok(Some(value)) => {
            process_map = bincode::deserialize::<ProcessMap>(&value).unwrap();
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
            db.put(&kernel_id, bincode::serialize(&process_map).unwrap())
                .unwrap();
        }
        Err(e) => {
            panic!("failed to load kernel state from db: {:?}", e);
        }
    }

    // bootstrap the distro processes into the node. TODO:
    // once we manage userspace sys packages onchain, stop
    // doing this and allow node operator to manually or auto-update
    // all their own userspace packages.
    bootstrap(
        &our_name,
        keypair,
        home_directory_path.clone(),
        runtime_extensions.clone(),
        &mut process_map,
    )
    .await
    .unwrap();

    Ok((process_map, db))
}

pub async fn state_sender(
    our_name: String,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_state: MessageReceiver,
    db: DB,
    home_directory_path: String,
) -> Result<(), anyhow::Error> {
    let db = Arc::new(db);

    let mut process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::new();

    loop {
        tokio::select! {
            Some(km) = recv_state.recv() => {
                if our_name != km.source.node {
                    println!(
                        "state: request must come from our_name={}, got: {}",
                        our_name, &km,
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

                let db_clone = db.clone();
                let send_to_loop = send_to_loop.clone();
                let send_to_terminal = send_to_terminal.clone();
                let our_name = our_name.clone();
                let home_directory_path = home_directory_path.clone();

                tokio::spawn(async move {
                    let mut queue_lock = queue.lock().await;
                    if let Some(km) = queue_lock.pop_front() {
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
                        }
                });
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
                error: format!("parse into StateAction failed: {:?}", e),
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
                    println!("get state error: {:?}", e);
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
                    println!("delete state error: {:?}", e);
                    return Err(StateError::RocksDBError {
                        action: "DeleteState".into(),
                        error: e.to_string(),
                    });
                }
            }
        }
        StateAction::Backup => {
            let checkpoint_dir = format!("{}/kernel/backup", &home_directory_path);

            if Path::new(&checkpoint_dir).exists() {
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

    if let Some(target) = rsvp.or_else(|| {
        expects_response.map(|_| Address {
            node: our_name.clone(),
            process: source.process.clone(),
        })
    }) {
        let response = KernelMessage {
            id,
            source: Address {
                node: our_name.clone(),
                process: STATE_PROCESS_ID.clone(),
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
    };

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
    keypair: Arc<signature::Ed25519KeyPair>,
    home_directory_path: String,
    runtime_extensions: Vec<(ProcessId, MessageSender, bool)>,
    process_map: &mut ProcessMap,
) -> Result<()> {
    // println!("bootstrapping node...\r");

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
            wit_version: None,
            on_exit: OnExit::Restart,
            capabilities: runtime_caps.clone(),
            public: false,
        });
    current_kernel.capabilities.extend(runtime_caps.clone());
    let current_net = process_map
        .entry(ProcessId::new(Some("net"), "distro", "sys"))
        .or_insert(PersistedProcess {
            wasm_bytes_handle: "".into(),
            wit_version: None,
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
                wit_version: None,
                on_exit: OnExit::Restart,
                capabilities: runtime_caps.clone(),
                public: runtime_module.2,
            });
        current.capabilities.extend(runtime_caps.clone());
    }

    let packages = get_zipped_packages().await;

    for (package_name, mut package) in packages.clone() {
        // special case tester: only load it in if in simulation mode
        if package_name == "tester" {
            #[cfg(not(feature = "simulation-mode"))]
            continue;
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

        // println!("fs: found package metadata: {:?}\r", package_metadata);

        let package_name = package_metadata["package"]
            .as_str()
            .expect("fs: metadata parse error: bad package name");

        let package_publisher = package_metadata["publisher"]
            .as_str()
            .expect("fs: metadata parse error: bad publisher name");

        // create a new package in VFS
        let our_drive_name = [package_name, package_publisher].join(":");
        let pkg_path = format!("{}/vfs/{}/pkg", &home_directory_path, &our_drive_name);
        fs::create_dir_all(&pkg_path)
            .await
            .expect("bootstrap vfs dir pkg creation failed!");

        let drive_path = format!("/{}/pkg", &our_drive_name);

        // save the zip itself inside pkg folder, for sharing with others
        let mut zip_file =
            fs::File::create(format!("{}/{}.zip", &pkg_path, &our_drive_name)).await?;
        let package_zip_bytes = package.clone().into_inner().into_inner();
        zip_file.write_all(package_zip_bytes).await?;

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
            let full_path = Path::new(&pkg_path).join(&file_path_str);

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
                params: serde_json::to_string(&serde_json::json!({
                    "kind": "read",
                    "drive": drive_path,
                }))
                .unwrap(),
            };
            requested_caps.insert(read_cap.clone(), sign_cap(read_cap, keypair.clone()));
            let write_cap = Capability {
                issuer: Address {
                    node: our_name.into(),
                    process: VFS_PROCESS_ID.clone(),
                },
                params: serde_json::to_string(&serde_json::json!({
                    "kind": "write",
                    "drive": drive_path,
                }))
                .unwrap(),
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
                    p.on_exit = entry.on_exit;
                    p.capabilities.extend(requested_caps);
                    p.public = public_process;
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(PersistedProcess {
                        wasm_bytes_handle: wasm_bytes_handle.clone(),
                        wit_version: None,
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
    for (package_name, mut package) in packages {
        // special case tester: only load it in if in simulation mode
        if package_name == "tester" {
            #[cfg(not(feature = "simulation-mode"))]
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

        // println!("fs: found package metadata: {:?}\r", package_metadata);

        let package_name = package_metadata["package"]
            .as_str()
            .expect("fs: metadata parse error: bad package name");

        let package_publisher = package_metadata["publisher"]
            .as_str()
            .expect("fs: metadata parse error: bad publisher name");

        // for each process-entry in manifest.json:
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
                                    .insert(cap.clone(), sign_cap(cap, keypair.clone()));
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
                                        process
                                            .capabilities
                                            .insert(cap.clone(), sign_cap(cap, keypair.clone()));
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
async fn get_zipped_packages() -> Vec<(String, zip::ZipArchive<std::io::Cursor<&'static [u8]>>)> {
    // println!("fs: reading distro packages...\r");

    let mut packages = Vec::new();

    for (package_name, bytes) in BOOTSTRAPPED_PROCESSES.iter() {
        if let Ok(zip) = zip::ZipArchive::new(std::io::Cursor::new(*bytes)) {
            // add to list of packages
            // println!("fs: found package: {}\r", package_name);
            packages.push((package_name.to_string(), zip));
        }
    }

    packages
}

impl From<std::io::Error> for StateError {
    fn from(err: std::io::Error) -> Self {
        StateError::IOError {
            error: err.to_string(),
        }
    }
}

fn make_error_message(our_name: String, km: &KernelMessage, error: StateError) -> KernelMessage {
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
                body: serde_json::to_vec(&StateResponse::Err(error)).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: None,
    }
}

fn process_to_vec(process: ProcessId) -> Vec<u8> {
    process.to_string().as_bytes().to_vec()
}
