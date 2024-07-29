use {
    crate::{
        kinode::process::main::OnchainMetadata,
        state::{AppStoreLogError, PackageState, SerializedState, State},
        KIMAP_ADDRESS, VFS_TIMEOUT,
    },
    alloy_primitives::keccak256,
    alloy_sol_types::SolEvent,
    kinode_process_lib::{
        eth, get_blob, get_state, http, kernel_types as kt, kimap, println, vfs, Address,
        LazyLoadBlob, PackageId, ProcessId, Request,
    },
    std::collections::HashSet,
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
                if state.kimap.address().to_string() == KIMAP_ADDRESS {
                    return State::from_serialized(our, state);
                } else {
                    println!(
                        "state contract address mismatch. rebuilding state! expected {}, got {}",
                        KIMAP_ADDRESS,
                        state.kimap.address().to_string()
                    );
                }
            }
            Err(e) => println!("failed to deserialize saved state, rebuilding: {e}"),
        }
    }
    State::new(our, provider).expect("state creation failed")
}

/// create the filter used for app store getLogs and subscription.
/// the app store exclusively looks for ~metadata-uri postings: if one is
/// observed, we then *query* for ~metadata-hash to verify the content
/// at the URI.
///
/// this means that ~metadata-hash should be *posted before or at the same time* as ~metadata-uri!
pub fn app_store_filter(state: &State) -> eth::Filter {
    let notes = vec![keccak256("~metadata-uri")];

    eth::Filter::new()
        .address(*state.kimap.address())
        .events([kimap::contract::Note::SIGNATURE])
        .topic3(notes)
}

/// create a filter to fetch app store event logs from chain and subscribe to new events
pub fn fetch_and_subscribe_logs(state: &mut State) {
    let filter = app_store_filter(state);
    // get past logs, subscribe to new ones.
    for log in fetch_logs(
        &state.kimap.provider,
        &filter.clone().from_block(state.last_saved_block),
    ) {
        if let Err(e) = state.ingest_contract_event(log, false) {
            println!("error ingesting log: {e:?}");
        };
    }
    state.update_listings();
    state.kimap.provider.subscribe_loop(1, filter);
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
                let hash = keccak_256_hash(&body.bytes);
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

/// generate a Keccak-256 hash string (with 0x prefix) of the metadata bytes
pub fn keccak_256_hash(bytes: &[u8]) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(bytes);
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
pub fn sha_256_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn fetch_package_manifest(
    package_id: &PackageId,
) -> anyhow::Result<Vec<kt::PackageManifestEntry>> {
    vfs_request(
        format!("/{package_id}/pkg/manifest.json"),
        vfs::VfsAction::Read,
    )
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
    // add to listings
    state.add_listing(package_id, metadata);

    // set the version hash for this new local package
    let our_version_hash = sha_256_hash(&bytes);

    let package_state = PackageState {
        mirrored_from: Some(state.our.node.clone()),
        our_version_hash,
        installed: false,
        verified: true, // sideloaded apps are implicitly verified because there is no "source" to verify against
        caps_approved: true, // TODO see if we want to auto-approve local installs
        manifest_hash: None, // generated in the add fn
        mirroring: mirror,
        auto_update: false, // can't auto-update a local package
    };
    state.add_downloaded_package(&package_id, package_state, Some(bytes))
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
    vfs_request(drive_name.clone(), vfs::VfsAction::CreateDrive)
        .send_and_await_response(VFS_TIMEOUT)??;

    // DELETE the /pkg folder in the package drive
    // in order to replace with the fresh one
    vfs_request(drive_name.clone(), vfs::VfsAction::RemoveDirAll)
        .send_and_await_response(VFS_TIMEOUT)??;

    // convert the zip to a new package drive
    let vfs::VfsResponse::Ok = serde_json::from_slice::<vfs::VfsResponse>(
        vfs_request(drive_name.clone(), vfs::VfsAction::AddZip)
            .blob(blob.clone())
            .send_and_await_response(VFS_TIMEOUT)??
            .body(),
    )?
    else {
        return Err(anyhow::anyhow!(
            "cannot add NewPackage: do not have capability to access vfs"
        ));
    };

    // save the zip file itself in VFS for sharing with other nodes
    // call it <package_id>.zip
    let zip_path = format!("{}/{}.zip", drive_name, package_id);
    vfs_request(zip_path, vfs::VfsAction::Write)
        .blob(blob)
        .send_and_await_response(VFS_TIMEOUT)??;

    let manifest_file = vfs::File {
        path: format!("/{}/pkg/manifest.json", package_id),
        timeout: VFS_TIMEOUT,
    };
    let manifest_bytes = manifest_file.read()?;
    Ok(keccak_256_hash(&manifest_bytes))
}

pub fn extract_api(package_id: &PackageId) -> anyhow::Result<bool> {
    // get `pkg/api.zip` if it exists
    if let vfs::VfsResponse::Read = serde_json::from_slice(
        vfs_request(format!("/{package_id}/pkg/api.zip"), vfs::VfsAction::Read)
            .send_and_await_response(VFS_TIMEOUT)??
            .body(),
    )? {
        // unzip api.zip into /api
        // blob inherited from Read request
        if let vfs::VfsResponse::Ok = serde_json::from_slice(
            vfs_request(format!("/{package_id}/pkg/api"), vfs::VfsAction::AddZip)
                .inherit(true)
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// given a `PackageId`, interact with VFS and kernel to get manifest,
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

        let process_id = ProcessId::new(
            Some(&entry.process_name),
            package_id.package(),
            package_id.publisher(),
        );

        // kill process if it already exists
        kernel_request(kt::KernelCommand::KillProcess(process_id.clone())).send()?;

        // read wasm file from VFS, bytes of which will be stored in blob
        if let Ok(vfs::VfsResponse::Err(e)) = serde_json::from_slice(
            vfs_request(&wasm_path, vfs::VfsAction::Read)
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        ) {
            return Err(anyhow::anyhow!("failed to read process file: {e}"));
        };

        // use inherited blob to initialize process in kernel
        let Ok(kt::KernelResponse::InitializedProcess) = serde_json::from_slice(
            kernel_request(kt::KernelCommand::InitializeProcess {
                id: process_id.clone(),
                wasm_bytes_handle: wasm_path,
                wit_version,
                on_exit: entry.on_exit.clone(),
                initial_capabilities: HashSet::new(),
                public: entry.public,
            })
            .inherit(true)
            .send_and_await_response(VFS_TIMEOUT)??
            .body(),
        ) else {
            return Err(anyhow::anyhow!("failed to initialize process"));
        };

        // build initial caps from manifest
        let mut requested_capabilities = parse_capabilities(our_node, &entry.request_capabilities);

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

        kernel_request(kt::KernelCommand::GrantCapabilities {
            target: process_id.clone(),
            capabilities: requested_capabilities,
        })
        .send()?;
    }

    // THEN, *after* all processes have been initialized, grant caps in manifest
    // this is done after initialization so that processes within a package
    // can grant capabilities to one another in the manifest.
    for entry in &manifest {
        let process_id = ProcessId::new(
            Some(&entry.process_name),
            package_id.package(),
            package_id.publisher(),
        );

        for value in &entry.grant_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        kernel_request(kt::KernelCommand::GrantCapabilities {
                            target: parsed_process_id,
                            capabilities: vec![kt::Capability {
                                issuer: Address {
                                    node: our_node.to_string(),
                                    process: process_id.clone(),
                                },
                                params: "\"messaging\"".into(),
                            }],
                        })
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
                                kernel_request(kt::KernelCommand::GrantCapabilities {
                                    target: parsed_process_id,
                                    capabilities: vec![kt::Capability {
                                        issuer: Address {
                                            node: our_node.to_string(),
                                            process: process_id.clone(),
                                        },
                                        params: params.to_string(),
                                    }],
                                })
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
            kernel_request(kt::KernelCommand::RunProcess(process_id))
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        ) else {
            return Err(anyhow::anyhow!("failed to start process"));
        };
    }
    Ok(())
}

/// given a `PackageId`, read its manifest, kill all processes declared in it,
/// then remove its drive in the virtual filesystem.
pub fn uninstall(package_id: &PackageId) -> anyhow::Result<()> {
    // the drive corresponding to the package we will be removing
    let drive_path = format!("/{package_id}/pkg");

    // get manifest.json from drive
    vfs_request(
        format!("{}/manifest.json", drive_path),
        vfs::VfsAction::Read,
    )
    .send_and_await_response(VFS_TIMEOUT)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let manifest = serde_json::from_slice::<Vec<kt::PackageManifestEntry>>(&blob.bytes)?;

    // reading from the package manifest, kill every process named
    for entry in &manifest {
        kernel_request(kt::KernelCommand::KillProcess(ProcessId::new(
            Some(&entry.process_name),
            package_id.package(),
            package_id.publisher(),
        )))
        .send()?;
    }

    // then, delete the drive
    vfs_request(drive_path, vfs::VfsAction::RemoveDirAll)
        .send_and_await_response(VFS_TIMEOUT)??;

    Ok(())
}

fn parse_capabilities(our_node: &str, caps: &Vec<serde_json::Value>) -> Vec<kt::Capability> {
    let mut requested_capabilities: Vec<kt::Capability> = vec![];
    for value in caps {
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
                    println!("manifest requested invalid cap: {value}");
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
                            println!("manifest requested invalid cap: {value}");
                        }
                    }
                }
            }
            val => {
                println!("manifest requested invalid cap: {val}");
                continue;
            }
        }
    }
    requested_capabilities
}

fn kernel_request(command: kt::KernelCommand) -> Request {
    Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&command).expect("failed to serialize VfsRequest"))
}

pub fn vfs_request<T>(path: T, action: vfs::VfsAction) -> Request
where
    T: Into<String>,
{
    Request::new().target(("our", "vfs", "distro", "sys")).body(
        serde_json::to_vec(&vfs::VfsRequest {
            path: path.into(),
            action,
        })
        .expect("failed to serialize VfsRequest"),
    )
}
