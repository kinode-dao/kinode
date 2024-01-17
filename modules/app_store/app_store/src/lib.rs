use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::*;
use kinode_process_lib::{call_init, println};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

mod ft_worker_lib;
use ft_worker_lib::{
    spawn_receive_transfer, spawn_transfer, FTWorkerCommand, FTWorkerResult, FileTransferContext,
};

/// App Store:
/// acts as both a local package manager and a protocol to share packages across the network.
/// packages are apps; apps are packages. we use an onchain app listing contract to determine
/// what apps are available to download and what node(s) to download them from.
///
/// once we know that list, we can request a package from a node and download it locally.
/// (we can also manually download an "untracked" package if we know its name and distributor node)
/// packages that are downloaded can then be installed!
///
/// installed packages can be managed:
/// - given permissions (necessary to complete install)
/// - uninstalled / suspended
/// - deleted ("undownloaded")
/// - set to automatically update if a new version is available (on by default)

//
// app store types
//

/// this process's saved state
#[derive(Debug, Serialize, Deserialize)]
struct State {
    pub packages: HashMap<PackageId, PackageState>,
    pub requested_packages: HashSet<PackageId>,
}

/// state of an individual package we have downloaded
#[derive(Debug, Serialize, Deserialize)]
struct PackageState {
    pub mirrored_from: NodeId,
    pub listing_data: PackageListing,
    pub mirroring: bool,   // are we serving this package to others?
    pub auto_update: bool, // if we get a listing data update, will we try to download it?
}

/// just a sketch of what we might get from chain
#[derive(Debug, Serialize, Deserialize)]
struct PackageListing {
    pub name: String,
    pub publisher: NodeId,
    pub description: Option<String>,
    pub website: Option<String>,
    pub version: kt::PackageVersion,
    pub version_hash: String, // sha256 hash of the package zip or whatever
}

//
// app store API
//

/// Remote requests, those sent between instantiations of this process
/// on different nodes, take this form. Will add more to enum in the future
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteRequest {
    /// no blob; request a package from a node
    /// remote node must return RemoteResponse::DownloadApproved,
    /// at which point requester can expect a FTWorkerRequest::Receive
    Download(PackageId),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteResponse {
    DownloadApproved,
    DownloadDenied, // TODO expand on why
}

/// Local requests take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalRequest {
    /// expects a zipped package as blob: create a new package from it
    /// if requested, will return a NewPackageResponse indicating success/failure
    NewPackage {
        package: PackageId,
        mirror: bool, // sets whether we will mirror this package
    },
    /// no blob; try to download a package from a specified node
    /// if requested, will return a DownloadResponse indicating success/failure
    Download {
        package: PackageId,
        install_from: NodeId,
    },
    /// no blob; select a downloaded package and install it
    /// if requested, will return an InstallResponse indicating success/failure
    Install(PackageId),
    /// Takes no blob; Select an installed package and uninstall it.
    /// This will kill the processes in the **manifest** of the package,
    /// but not the processes that were spawned by those processes! Take
    /// care to kill those processes yourself. This will also delete the drive
    /// containing the source code for this package. This does not guarantee
    /// that other data created by this package will be removed from places such
    /// as the key-value store.
    Uninstall(PackageId),
}

/// Local responses take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalResponse {
    NewPackageResponse(NewPackageResponse),
    DownloadResponse(DownloadResponse),
    InstallResponse(InstallResponse),
    UninstallResponse(UninstallResponse),
}

// TODO for all: expand these to elucidate why something failed
// these are locally-given responses to local requests

#[derive(Debug, Serialize, Deserialize)]
pub enum NewPackageResponse {
    Success,
    Failure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DownloadResponse {
    Started,
    Failure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum InstallResponse {
    Success,
    Failure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum UninstallResponse {
    Success,
    Failure,
}

// internal types

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    FTWorkerCommand(FTWorkerCommand),
    FTWorkerResult(FTWorkerResult),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    RemoteResponse(RemoteResponse),
    FTWorkerResult(FTWorkerResult),
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestCap {
    process: String,
    params: Value,
}

// /m our@main:app_store:ben.os {"Download": {"package": {"package_name": "sdapi", "publisher_node": "benjammin.os"}, "install_from": "testnode107.os"}}
// /m our@main:app_store:ben.os {"Install": {"package_name": "sdapi", "publisher_node": "benjammin.os"}}

call_init!(init);
fn init(our: Address) {
    println!("{}: started", our.package());

    // load in our saved state or initalize a new one if none exists
    let mut state = get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)).unwrap_or(State {
        packages: HashMap::new(),
        requested_packages: HashSet::new(),
    });

    loop {
        match await_message() {
            Err(send_error) => {
                println!("app store: got network error: {send_error:?}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &message) {
                    println!("app store: error handling message: {:?}", e)
                }
            }
        }
    }
}

fn handle_message(our: &Address, mut state: &mut State, message: &Message) -> anyhow::Result<()> {
    match message {
        Message::Request {
            source,
            expects_response,
            body,
            ..
        } => {
            match &serde_json::from_slice::<Req>(&body)? {
                Req::LocalRequest(local_request) => {
                    if our.node != source.node {
                        return Err(anyhow::anyhow!("local request from non-local node"));
                    }
                    let resp = handle_local_request(&our, local_request, &mut state);
                    if expects_response.is_some() {
                        Response::new().body(serde_json::to_vec(&resp)?).send()?;
                    }
                }
                Req::RemoteRequest(remote_request) => {
                    let resp = handle_remote_request(&our, &source, remote_request, &mut state);
                    if expects_response.is_some() {
                        Response::new().body(serde_json::to_vec(&resp)?).send()?;
                    }
                }
                Req::FTWorkerResult(FTWorkerResult::ReceiveSuccess(name)) => {
                    // do with file what you'd like here
                    println!("app store: successfully received {:?}", name);
                    // remove leading / and .zip from file name to get package ID
                    let package_id = match name[1..].trim_end_matches(".zip").parse::<PackageId>() {
                        Ok(package_id) => package_id,
                        Err(e) => {
                            println!("app store: bad package filename: {}", name);
                            return Err(anyhow::anyhow!(e));
                        }
                    };
                    // only install the app if we actually requested it
                    if state.requested_packages.remove(&package_id) {
                        // auto-take zip from blob and request ourself with New
                        Request::new()
                            .target(our.clone())
                            .inherit(true)
                            .body(serde_json::to_vec(&Req::LocalRequest(
                                LocalRequest::NewPackage {
                                    package: package_id,
                                    mirror: true, // can turn off auto-mirroring
                                },
                            ))?)
                            .send()?;
                        crate::set_state(&bincode::serialize(state)?);
                    }
                }
                Req::FTWorkerResult(r) => {
                    println!("app store: got ft_worker result: {r:?}");
                }
                Req::FTWorkerCommand(_) => {
                    spawn_receive_transfer(&our, &body)?;
                }
            }
        }
        Message::Response { body, context, .. } => match &serde_json::from_slice::<Resp>(&body)? {
            Resp::RemoteResponse(remote_response) => match remote_response {
                RemoteResponse::DownloadApproved => {
                    println!("app store: download approved");
                }
                RemoteResponse::DownloadDenied => {
                    println!("app store: could not download package from that node!");
                }
            },
            Resp::FTWorkerResult(ft_worker_result) => {
                let context =
                    serde_json::from_slice::<FileTransferContext>(&context.as_ref().unwrap())?;
                match ft_worker_result {
                    FTWorkerResult::SendSuccess => {
                        println!(
                            "app store: successfully shared app {} in {:.4}s",
                            context.file_name,
                            std::time::SystemTime::now()
                                .duration_since(context.start_time)
                                .unwrap()
                                .as_secs_f64(),
                        );
                    }
                    e => return Err(anyhow::anyhow!("app store: ft_worker gave us {e:?}")),
                }
            }
        },
    }
    Ok(())
}

/// only `our.node` can call this
fn handle_local_request(our: &Address, request: &LocalRequest, state: &mut State) -> LocalResponse {
    match request {
        LocalRequest::NewPackage { package, mirror } => {
            match handle_new_package(our, package, *mirror, state) {
                Ok(()) => LocalResponse::NewPackageResponse(NewPackageResponse::Success),
                Err(_) => LocalResponse::NewPackageResponse(NewPackageResponse::Failure),
            }
        }
        LocalRequest::Download {
            package,
            install_from,
        } => LocalResponse::DownloadResponse(
            match Request::new()
                .target((install_from.as_str(), our.process.clone()))
                .inherit(true)
                .body(serde_json::to_vec(&RemoteRequest::Download(package.clone())).unwrap())
                .send_and_await_response(5)
            {
                Ok(Ok(Message::Response { body, .. })) => {
                    match serde_json::from_slice::<Resp>(&body) {
                        Ok(Resp::RemoteResponse(RemoteResponse::DownloadApproved)) => {
                            state.requested_packages.insert(package.clone());
                            crate::set_state(&bincode::serialize(&state).unwrap());
                            DownloadResponse::Started
                        }
                        _ => DownloadResponse::Failure,
                    }
                }
                _ => DownloadResponse::Failure,
            },
        ),
        LocalRequest::Install(package) => match handle_install(our, package) {
            Ok(()) => LocalResponse::InstallResponse(InstallResponse::Success),
            Err(_) => LocalResponse::InstallResponse(InstallResponse::Failure),
        },
        LocalRequest::Uninstall(package) => match handle_uninstall(package) {
            Ok(()) => LocalResponse::UninstallResponse(UninstallResponse::Success),
            Err(_) => LocalResponse::UninstallResponse(UninstallResponse::Failure),
        },
    }
}

fn handle_new_package(
    our: &Address,
    package: &PackageId,
    mirror: bool,
    state: &mut State,
) -> anyhow::Result<()> {
    let Some(mut blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let drive = format!("/{}/pkg", package);

    // create a new drive for this package in VFS
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: drive.clone(),
            action: vfs::VfsAction::CreateDrive,
        })?)
        .send_and_await_response(5)??;

    // produce the version hash for this new package
    let mut hasher = sha2::Sha256::new();
    hasher.update(&blob.bytes);
    let version_hash = format!("{:x}", hasher.finalize());

    // add zip bytes
    blob.mime = Some("application/zip".to_string());
    let response = Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: drive.clone(),
            action: vfs::VfsAction::AddZip,
        })?)
        .blob(blob.clone())
        .send_and_await_response(5)??;
    let vfs_body = serde_json::from_slice::<serde_json::Value>(response.body())?;
    if vfs_body == serde_json::json!({"Err": "NoCap"}) {
        return Err(anyhow::anyhow!(
            "cannot add NewPackage: do not have capability to access vfs"
        ));
    }

    // save the zip file itself in VFS for sharing with other nodes
    // call it <package>.zip
    let zip_path = format!("{}/{}.zip", drive.clone(), package);
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .inherit(true)
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: zip_path,
            action: vfs::VfsAction::Write,
        })?)
        .blob(blob)
        .send_and_await_response(5)??;
    let metadata_path = format!("{}/metadata.json", drive.clone());

    // now, read the pkg contents to create our own listing and state,
    // such that we can mirror this package to others.
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: metadata_path,
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no metadata found!"));
    };

    let metadata = String::from_utf8(blob.bytes)?;
    let metadata = serde_json::from_str::<kt::PackageMetadata>(&metadata)?;

    let listing_data = PackageListing {
        name: metadata.package,
        publisher: our.node.clone(),
        description: metadata.description,
        website: metadata.website,
        version: metadata.version,
        version_hash,
    };
    let package_state = PackageState {
        mirrored_from: our.node.clone(),
        listing_data,
        mirroring: mirror,
        auto_update: true,
    };
    state.packages.insert(package.clone(), package_state);
    crate::set_state(&bincode::serialize(state).unwrap());
    Ok(())
}

fn handle_install(our: &Address, package: &PackageId) -> anyhow::Result<()> {
    let drive_path = format!("/{}/pkg", package);
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/manifest.json", drive_path),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let manifest = String::from_utf8(blob.bytes)?;
    let manifest = serde_json::from_str::<Vec<kt::PackageManifestEntry>>(&manifest)?;
    // always grant read/write to their drive, which we created for them
    let Some(read_cap) = get_capability(
        &Address::new(&our.node, ("vfs", "distro", "sys")),
        &serde_json::to_string(&serde_json::json!({
            "kind": "read",
            "drive": drive_path,
        }))?,
    ) else {
        return Err(anyhow::anyhow!("app store: no read cap"));
    };
    let Some(write_cap) = get_capability(
        &Address::new(&our.node, ("vfs", "distro", "sys")),
        &serde_json::to_string(&serde_json::json!({
            "kind": "write",
            "drive": drive_path,
        }))?,
    ) else {
        return Err(anyhow::anyhow!("app store: no write cap"));
    };
    let Some(networking_cap) = get_capability(
        &Address::new(&our.node, ("kernel", "distro", "sys")),
        &"\"network\"".to_string(),
    ) else {
        return Err(anyhow::anyhow!("app store: no net cap"));
    };
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
        // build initial caps
        let mut initial_capabilities: HashSet<kt::Capability> = HashSet::new();
        if entry.request_networking {
            initial_capabilities.insert(kt::de_wit_capability(networking_cap.clone()));
        }
        initial_capabilities.insert(kt::de_wit_capability(read_cap.clone()));
        initial_capabilities.insert(kt::de_wit_capability(write_cap.clone()));
        let process_id = format!("{}:{}", entry.process_name, package);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("app store: invalid process id!"));
        };
        // kill process if it already exists
        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::KillProcess(
                parsed_new_process_id.clone(),
            ))?)
            .send()?;

        let _bytes_response = Request::new()
            .target(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: wasm_path.clone(),
                action: vfs::VfsAction::Read,
            })?)
            .send_and_await_response(5)??;
        for value in &entry.request_capabilities {
            let mut capability = None;
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        capability = get_capability(
                            &Address {
                                node: our.node.clone(),
                                process: parsed_process_id.clone(),
                            },
                            "\"messaging\"".into(),
                        );
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
                                if params.to_string() == "\"root\"" {
                                    println!(
                                        "app-store: app requested root capability, ignoring"
                                    );
                                    continue;
                                }

                                capability = get_capability(
                                    &Address {
                                        node: our.node.clone(),
                                        process: parsed_process_id.clone(),
                                    },
                                    &params.to_string(),
                                );
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
            if let Some(cap) = capability {
                initial_capabilities.insert(kt::de_wit_capability(cap));
            } else {
                println!(
                    "app-store: no cap: {}, for {} to request!",
                    value.to_string(),
                    package
                );
            }
        }
        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                id: parsed_new_process_id.clone(),
                wasm_bytes_handle: wasm_path,
                wit_version: None,
                on_exit: entry.on_exit.clone(),
                initial_capabilities,
                public: entry.public,
            })?)
            .inherit(true)
            .send_and_await_response(5)??;
        for value in &entry.grant_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        let _ = Request::new()
                            .target(("our", "kernel", "distro", "sys"))
                            .body(
                                serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                                    target: parsed_process_id,
                                    capabilities: vec![kt::Capability {
                                        issuer: Address {
                                            node: our.node.clone(),
                                            process: parsed_new_process_id.clone(),
                                        },
                                        params: "\"messaging\"".into(),
                                    }],
                                })
                                .unwrap(),
                            )
                            .send()?;
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
                                let _ = Request::new()
                                    .target(("our", "kernel", "distro", "sys"))
                                    .body(
                                        serde_json::to_vec(
                                            &kt::KernelCommand::GrantCapabilities {
                                                target: parsed_process_id,
                                                capabilities: vec![kt::Capability {
                                                    issuer: Address {
                                                        node: our.node.clone(),
                                                        process: parsed_new_process_id.clone(),
                                                    },
                                                    params: params.to_string(),
                                                }],
                                            },
                                        )
                                        .unwrap(),
                                    )
                                    .send()?;
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
        }
        Request::new()
            .target(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
                parsed_new_process_id,
            ))?)
            .send_and_await_response(5)??;
    }
    Ok(())
}

fn handle_uninstall(package: &PackageId) -> anyhow::Result<()> {
    let drive_path = format!("/{}/pkg", package);
    Request::new()
        .target(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/manifest.json", drive_path),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let manifest = String::from_utf8(blob.bytes)?;
    let manifest = serde_json::from_str::<Vec<kt::PackageManifestEntry>>(&manifest)?;
    // reading from the package manifest, kill every process
    for entry in &manifest {
        let process_id = format!("{}:{}", entry.process_name, package);
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
        .send_and_await_response(5)??;
    Ok(())
}

fn handle_remote_request(
    our: &Address,
    source: &Address,
    request: &RemoteRequest,
    state: &mut State,
) -> Resp {
    match request {
        RemoteRequest::Download(package) => {
            let Some(package_state) = state.packages.get(&package) else {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
            };
            if !package_state.mirroring {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
            }
            // get the .zip from VFS and attach as blob to response
            let file_path = format!("/{}/pkg/{}.zip", package, package);
            let Ok(Ok(_)) = Request::new()
                .target(("our", "vfs", "distro", "sys"))
                .body(
                    serde_json::to_vec(&vfs::VfsRequest {
                        path: file_path,
                        action: vfs::VfsAction::Read,
                    })
                    .unwrap(),
                )
                .send_and_await_response(5)
            else {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
            };
            // transfer will *inherit* the blob bytes we receive from VFS
            let file_name = format!("/{}.zip", package);
            match spawn_transfer(&our, &file_name, None, 60, &source) {
                Ok(()) => Resp::RemoteResponse(RemoteResponse::DownloadApproved),
                Err(_e) => Resp::RemoteResponse(RemoteResponse::DownloadDenied),
            }
        }
    }
}
