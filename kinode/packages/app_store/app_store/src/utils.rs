use {
    crate::kinode::process::main::OnchainMetadata,
    crate::state::{AppStoreLogError, PackageState, SerializedState, State},
    crate::{CONTRACT_ADDRESS, EVENTS, VFS_TIMEOUT},
    kinode_process_lib::{
        eth, get_blob, get_state, http, kernel_types as kt, println, vfs, Address, LazyLoadBlob,
        PackageId, ProcessId, Request,
    },
    std::collections::HashSet,
    std::str::FromStr,
};

// quite annoyingly, we must convert from our gen'd version of PackageId
// to the process_lib's gen'd version. this is in order to access custom
// Impls that we want to use
impl crate::kinode::process::main::PackageId {
    pub fn to_process_lib(self) -> PackageId {
        PackageId {
            package_name: self.package_name,
            publisher_node: self.publisher_node,
        }
    }
    pub fn from_process_lib(package_id: PackageId) -> Self {
        Self {
            package_name: package_id.package_name,
            publisher_node: package_id.publisher_node,
        }
    }
}

// less annoying but still bad
impl OnchainMetadata {
    pub fn to_erc721_metadata(self) -> kt::Erc721Metadata {
        use kt::Erc721Properties;
        kt::Erc721Metadata {
            name: self.name,
            description: self.description,
            image: self.image,
            external_url: self.external_url,
            animation_url: self.animation_url,
            properties: Erc721Properties {
                package_name: self.properties.package_name,
                publisher: self.properties.publisher,
                current_version: self.properties.current_version,
                mirrors: self.properties.mirrors,
                code_hashes: self.properties.code_hashes.into_iter().collect(),
                license: self.properties.license,
                screenshots: self.properties.screenshots,
                wit_version: self.properties.wit_version,
                dependencies: self.properties.dependencies,
            },
        }
    }
}

/// fetch state from disk or create a new one if that fails
pub fn fetch_state(our: Address, provider: eth::Provider) -> State {
    if let Some(state_bytes) = get_state() {
        match serde_json::from_slice::<SerializedState>(&state_bytes) {
            Ok(state) => {
                if state.contract_address == CONTRACT_ADDRESS {
                    return State::from_serialized(our, provider, state);
                } else {
                    println!(
                        "state contract address mismatch! expected {}, got {}",
                        CONTRACT_ADDRESS, state.contract_address
                    );
                }
            }
            Err(e) => println!("failed to deserialize saved state: {e}"),
        }
    }
    State::new(our, provider, CONTRACT_ADDRESS.to_string()).expect("state creation failed")
}

pub fn app_store_filter(state: &State) -> eth::Filter {
    eth::Filter::new()
        .address(eth::Address::from_str(&state.contract_address).unwrap())
        .from_block(state.last_saved_block - 1)
        .events(EVENTS)
}

/// create a filter to fetch app store event logs from chain and subscribe to new events
pub fn fetch_and_subscribe_logs(state: &mut State) {
    let filter = app_store_filter(state);
    // get past logs, subscribe to new ones.
    for log in fetch_logs(&state.provider, &filter) {
        if let Err(e) = state.ingest_contract_event(log, false) {
            println!("error ingesting log: {e:?}");
        };
    }
    state.update_listings();
    subscribe_to_logs(&state.provider, filter);
}

/// subscribe to logs from the chain with a given filter
pub fn subscribe_to_logs(eth_provider: &eth::Provider, filter: eth::Filter) {
    loop {
        match eth_provider.subscribe(1, filter.clone()) {
            Ok(()) => break,
            Err(_) => {
                println!("failed to subscribe to chain! trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        }
    }
    println!("subscribed to logs successfully");
}

/// fetch logs from the chain with a given filter
fn fetch_logs(eth_provider: &eth::Provider, filter: &eth::Filter) -> Vec<eth::Log> {
    loop {
        match eth_provider.get_logs(filter) {
            Ok(res) => return res,
            Err(_) => {
                println!("failed to fetch logs! trying again in 5s...");
                std::thread::sleep(std::time::Duration::from_secs(5));
                continue;
            }
        }
    }
}

/// fetch metadata from url and verify it matches metadata_hash
pub fn fetch_metadata_from_url(
    metadata_url: &str,
    metadata_hash: &str,
    timeout: u64,
) -> Result<kt::Erc721Metadata, AppStoreLogError> {
    if let Ok(url) = url::Url::parse(metadata_url) {
        if let Ok(_) =
            http::send_request_await_response(http::Method::GET, url, None, timeout, vec![])
        {
            if let Some(body) = get_blob() {
                let hash = generate_metadata_hash(&body.bytes);
                if &hash == metadata_hash {
                    return Ok(serde_json::from_slice::<kt::Erc721Metadata>(&body.bytes)
                        .map_err(|_| AppStoreLogError::MetadataNotFound)?);
                } else {
                    return Err(AppStoreLogError::MetadataHashMismatch);
                }
            }
        }
    }
    Err(AppStoreLogError::MetadataNotFound)
}

/// generate a Keccak-256 hash of the metadata bytes
pub fn generate_metadata_hash(metadata: &[u8]) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(metadata);
    format!("0x{:x}", hasher.finalize())
}

/// generate a Keccak-256 hash of the package name and publisher (match onchain)
pub fn generate_package_hash(name: &str, publisher_dnswire: &[u8]) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update([name.as_bytes(), publisher_dnswire].concat());
    let hash = hasher.finalize();
    format!("0x{:x}", hash)
}

/// generate a SHA-256 hash of the zip bytes to act as a version hash
pub fn generate_version_hash(zip_bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(zip_bytes);
    format!("{:x}", hasher.finalize())
}

pub fn fetch_package_manifest(
    package_id: &PackageId,
) -> anyhow::Result<Vec<kt::PackageManifestEntry>> {
    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("/{package_id}/pkg/manifest.json"),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(VFS_TIMEOUT)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    Ok(serde_json::from_slice::<Vec<kt::PackageManifestEntry>>(
        &blob.bytes,
    )?)
}

pub fn new_package(
    package_id: &PackageId,
    state: &mut State,
    metadata: kt::Erc721Metadata,
    mirror: bool,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    // set the version hash for this new local package
    let our_version = generate_version_hash(&bytes);

    let package_state = PackageState {
        mirrored_from: Some(state.our.node.clone()),
        our_version,
        installed: false,
        verified: true, // side loaded apps are implicitly verified because there is no "source" to verify against
        caps_approved: true, // TODO see if we want to auto-approve local installs
        manifest_hash: None, // generated in the add fn
        mirroring: mirror,
        auto_update: false, // can't auto-update a local package
        metadata: Some(metadata),
    };
    let Ok(()) = state.add_downloaded_package(&package_id, package_state, Some(bytes)) else {
        return Err(anyhow::anyhow!("failed to add package"));
    };

    let drive_path = format!("/{package_id}/pkg");
    let result = Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(
            serde_json::to_vec(&vfs::VfsRequest {
                path: format!("{}/api", drive_path),
                action: vfs::VfsAction::Metadata,
            })
            .unwrap(),
        )
        .send_and_await_response(VFS_TIMEOUT);
    if let Ok(Ok(_)) = result {
        state.downloaded_apis.insert(package_id.to_owned());
    };
    Ok(())
}

/// create a new package drive in VFS and add the package zip to it.
/// if an `api.zip` is present, unzip and stow in `/api`.
/// returns a string representing the manifest hash of the package
/// and a bool returning whether or not an api was found and unzipped.
pub fn create_package_drive(
    package_id: &PackageId,
    package_bytes: Vec<u8>,
) -> anyhow::Result<String> {
    let drive_name = format!("/{package_id}/pkg");
    let blob = LazyLoadBlob {
        mime: Some("application/zip".to_string()),
        bytes: package_bytes,
    };

    // create a new drive for this package in VFS
    // this is possible because we have root access
    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: drive_name.clone(),
            action: vfs::VfsAction::CreateDrive,
        })?)
        .send_and_await_response(VFS_TIMEOUT)??;

    // convert the zip to a new package drive
    let response = Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: drive_name.clone(),
            action: vfs::VfsAction::AddZip,
        })?)
        .blob(blob.clone())
        .send_and_await_response(VFS_TIMEOUT)??;
    let vfs::VfsResponse::Ok = serde_json::from_slice::<vfs::VfsResponse>(response.body())? else {
        return Err(anyhow::anyhow!(
            "cannot add NewPackage: do not have capability to access vfs"
        ));
    };

    // save the zip file itself in VFS for sharing with other nodes
    // call it <package_id>.zip
    let zip_path = format!("{}/{}.zip", drive_name, package_id);
    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: zip_path,
            action: vfs::VfsAction::Write,
        })?)
        .blob(blob)
        .send_and_await_response(VFS_TIMEOUT)??;

    let manifest_file = vfs::File {
        path: format!("/{}/pkg/manifest.json", package_id),
        timeout: VFS_TIMEOUT,
    };
    let manifest_bytes = manifest_file.read()?;
    Ok(generate_metadata_hash(&manifest_bytes))
}

pub fn extract_api(package_id: &PackageId) -> anyhow::Result<bool> {
    // get `pkg/api.zip` if it exists
    let api_response = Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("/{package_id}/pkg/api.zip"),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(VFS_TIMEOUT)??;
    if let Ok(vfs::VfsResponse::Read) = serde_json::from_slice(api_response.body()) {
        // unzip api.zip into /api
        // blob inherited from Read request
        let response = Request::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: format!("/{package_id}/pkg/api"),
                action: vfs::VfsAction::AddZip,
            })?)
            .inherit(true)
            .send_and_await_response(VFS_TIMEOUT)??;
        if let Ok(vfs::VfsResponse::Ok) = serde_json::from_slice(response.body()) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// given a package id, interact with VFS and kernel to get manifest,
/// grant the capabilities in manifest, then initialize and start
/// the processes in manifest.
///
/// this will also grant the process read/write access to their drive,
/// which we can only do if we were the process to create that drive.
/// note also that each capability will only be granted if we, the process
/// using this function, own that capability ourselves.
pub fn install(
    package_id: &PackageId,
    our_node: &str,
    wit_version: Option<u32>,
) -> anyhow::Result<()> {
    // get the package manifest
    let drive_path = format!("/{package_id}/pkg");
    let manifest = fetch_package_manifest(package_id)?;

    // first, for each process in manifest, initialize it
    // then, once all have been initialized, grant them requested caps
    // and finally start them.
    for entry in &manifest {
        let wasm_path = if entry.process_wasm_path.starts_with("/") {
            entry.process_wasm_path.clone()
        } else {
            format!("/{}", entry.process_wasm_path)
        };
        let wasm_path = format!("{}{}", drive_path, wasm_path);
        let process_id = format!("{}:{}", entry.process_name, package_id);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("invalid process id!"));
        };
        // kill process if it already exists
        Request::to(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::KillProcess(
                parsed_new_process_id.clone(),
            ))?)
            .send()?;

        if let Ok(vfs::VfsResponse::Err(_)) = serde_json::from_slice(
            Request::to(("our", "vfs", "distro", "sys"))
                .body(serde_json::to_vec(&vfs::VfsRequest {
                    path: wasm_path.clone(),
                    action: vfs::VfsAction::Read,
                })?)
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        ) {
            return Err(anyhow::anyhow!("failed to read process file"));
        };

        let Ok(kt::KernelResponse::InitializedProcess) = serde_json::from_slice(
            Request::new()
                .target(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                    id: parsed_new_process_id.clone(),
                    wasm_bytes_handle: wasm_path,
                    wit_version,
                    on_exit: entry.on_exit.clone(),
                    initial_capabilities: HashSet::new(),
                    public: entry.public,
                })?)
                .inherit(true)
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        ) else {
            return Err(anyhow::anyhow!("failed to initialize process"));
        };
        // build initial caps
        let mut requested_capabilities: Vec<kt::Capability> = vec![];
        for value in &entry.request_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        requested_capabilities.push(kt::Capability {
                            issuer: Address {
                                node: our_node.to_string(),
                                process: parsed_process_id.clone(),
                            },
                            params: "\"messaging\"".into(),
                        });
                    } else {
                        println!("{process_id} manifest requested invalid cap: {value}");
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                requested_capabilities.push(kt::Capability {
                                    issuer: Address {
                                        node: our_node.to_string(),
                                        process: parsed_process_id.clone(),
                                    },
                                    params: params.to_string(),
                                });
                            } else {
                                println!("{process_id} manifest requested invalid cap: {value}");
                            }
                        }
                    }
                }
                val => {
                    println!("{process_id} manifest requested invalid cap: {val}");
                    continue;
                }
            }
        }

        if entry.request_networking {
            requested_capabilities.push(kt::Capability {
                issuer: Address::new(our_node, ("kernel", "distro", "sys")),
                params: "\"network\"".to_string(),
            });
        }

        // always grant read/write to their drive, which we created for them
        requested_capabilities.push(kt::Capability {
            issuer: Address::new(our_node, ("vfs", "distro", "sys")),
            params: serde_json::json!({
                "kind": "read",
                "drive": drive_path,
            })
            .to_string(),
        });
        requested_capabilities.push(kt::Capability {
            issuer: Address::new(our_node, ("vfs", "distro", "sys")),
            params: serde_json::json!({
                "kind": "write",
                "drive": drive_path,
            })
            .to_string(),
        });

        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                target: parsed_new_process_id.clone(),
                capabilities: requested_capabilities,
            })?)
            .send()?;
    }

    // THEN, *after* all processes have been initialized, grant caps in manifest
    // this is done after initialization so that processes within a package
    // can grant capabilities to one another in the manifest.
    for entry in &manifest {
        let process_id = format!("{}:{}", entry.process_name, package_id);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("invalid process id!"));
        };
        for value in &entry.grant_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        Request::to(("our", "kernel", "distro", "sys"))
                            .body(
                                serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                                    target: parsed_process_id,
                                    capabilities: vec![kt::Capability {
                                        issuer: Address {
                                            node: our_node.to_string(),
                                            process: parsed_new_process_id.clone(),
                                        },
                                        params: "\"messaging\"".into(),
                                    }],
                                })
                                .unwrap(),
                            )
                            .send()?;
                    } else {
                        println!("{process_id} manifest tried to grant invalid cap: {value}");
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                Request::to(("our", "kernel", "distro", "sys"))
                                    .body(serde_json::to_vec(
                                        &kt::KernelCommand::GrantCapabilities {
                                            target: parsed_process_id,
                                            capabilities: vec![kt::Capability {
                                                issuer: Address {
                                                    node: our_node.to_string(),
                                                    process: parsed_new_process_id.clone(),
                                                },
                                                params: params.to_string(),
                                            }],
                                        },
                                    )?)
                                    .send()?;
                            }
                        }
                    } else {
                        println!("{process_id} manifest tried to grant invalid cap: {value}");
                    }
                }
                val => {
                    println!("{process_id} manifest tried to grant invalid cap: {val}");
                    continue;
                }
            }
        }

        let Ok(kt::KernelResponse::StartedProcess) = serde_json::from_slice(
            Request::to(("our", "kernel", "distro", "sys"))
                .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
                    parsed_new_process_id,
                ))?)
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        ) else {
            return Err(anyhow::anyhow!("failed to start process"));
        };
    }
    Ok(())
}

pub fn uninstall(package_id: &PackageId) -> anyhow::Result<()> {
    let drive_path = format!("/{package_id}/pkg");
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/manifest.json", drive_path),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(VFS_TIMEOUT)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let manifest = String::from_utf8(blob.bytes)?;
    let manifest = serde_json::from_str::<Vec<kt::PackageManifestEntry>>(&manifest)?;
    // reading from the package manifest, kill every process
    for entry in &manifest {
        let process_id = format!("{}:{}", entry.process_name, package_id);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            continue;
        };
        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::KillProcess(
                parsed_new_process_id,
            ))?)
            .send()?;
    }
    // then, delete the drive
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: drive_path,
            action: vfs::VfsAction::RemoveDirAll,
        })?)
        .send_and_await_response(VFS_TIMEOUT)??;

    Ok(())
}
