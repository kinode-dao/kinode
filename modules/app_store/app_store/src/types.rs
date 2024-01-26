use alloy_rpc_types::Log;
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::eth::EthAddress;
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

sol! {
    event AppRegistered(uint256,string,string,uint256);
    event AppUnlisted(uint256);
    event AppMetadataUpdated(uint256,uint256);
    event Transfer(address,address,uint256);
}

//
// app store types
//

pub type PackageHash = String;

/// this process's saved state
#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    /// the address of the contract we are using to read package listings
    /// this is set by runtime distro at boot-time
    pub contract_address: Option<String>,
    /// the last block at which we saved the state of the listings to disk.
    /// we don't want to save the state every time we get a new listing,
    /// so we only save it every so often and then mark the block at which
    /// that last occurred here. when we boot, we can read logs starting
    /// from this block and rebuild latest state.
    pub last_saved_block: u64,
    /// we keep the full state of the package manager here, calculated from
    /// the listings contract logs. in the future, we'll offload this and
    /// only track a certain number of packages...
    listed_packages: HashMap<PackageHash, PackageListing>,
    /// we keep the full state of the packages we have downloaded here.
    /// in order to keep this synchronized with our filesystem, we will
    /// ingest apps on disk if we have to rebuild our state. this is also
    /// updated every time we download, create, or uninstall a package.
    downloaded_packages: HashMap<PackageId, PackageState>,
}

/// state of an individual package we have downloaded
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageState {
    /// the node we last downloaded the package from
    /// this is "us" if we don't know the source (usually cause it's a local install)
    pub mirrored_from: Option<NodeId>,
    /// the version of the package we have downloaded
    pub our_version: String,
    /// are we serving this package to others?
    pub mirroring: bool,
    /// if we get a listing data update, will we try to download it?
    pub auto_update: bool,
    pub metadata: Option<OnchainPackageMetadata>,
}

/// listing information derived from metadata hash in listing event
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageListing {
    pub owner: EthAddress,
    pub name: String,
    pub publisher: NodeId,
    pub metadata_hash: String,
    pub metadata: Option<OnchainPackageMetadata>,
}

/// metadata derived from metadata hash in listing event
#[derive(Debug, Serialize, Deserialize)]
pub struct OnchainPackageMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub version: Option<String>,
    pub license: Option<String>,
    pub website: Option<String>,
    pub mirrors: Vec<NodeId>,
    pub versions: Vec<String>,
}

pub struct RequestedPackage {
    pub mirror: bool,
    pub auto_update: bool,
    // if none, we're requesting the latest version onchain
    pub desired_version_hash: Option<String>,
}

impl State {
    /// To create a new state, we populate the downloaded_packages map
    /// with all packages parseable from our filesystem.
    pub fn new() -> Self {
        let mut state = State {
            contract_address: None,
            last_saved_block: 1,
            listed_packages: HashMap::new(),
            downloaded_packages: HashMap::new(),
        };
        let _ = state.populate_packages_from_filesystem();
        state
    }

    pub fn get_listing(&self, package_id: &PackageId) -> Option<&PackageListing> {
        let package_hash = generate_package_hash(package_id.package(), package_id.publisher());
        self.listed_packages.get(&package_hash)
    }

    pub fn get_downloaded_package(&self, package: &PackageId) -> Option<&PackageState> {
        self.downloaded_packages.get(package)
    }

    /// Done in response to any new onchain listing update other than 'delete'
    fn update_listing(&mut self, listing: PackageListing) {
        self.listed_packages.insert(
            generate_package_hash(&listing.name, &listing.publisher),
            listing,
        );
    }

    /// Done in response to an onchain listing update of 'delete'
    fn delete_listing(&mut self, package_id: &PackageId) {
        let package_hash = generate_package_hash(package_id.package(), package_id.publisher());
        self.listed_packages.remove(&package_hash);
    }

    /// saves state
    pub fn populate_packages_from_filesystem(&mut self) -> anyhow::Result<()> {
        let Message::Response { body, .. } = Request::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: "/".to_string(),
                action: vfs::VfsAction::ReadDir,
            })?)
            .send_and_await_response(3)?? else {
                return Err(anyhow::anyhow!("vfs: bad response"));
            };
        let response = serde_json::from_slice::<vfs::VfsResponse>(&body)?;
        let vfs::VfsResponse::ReadDir(entries) = response else {
            return Err(anyhow::anyhow!("vfs: unexpected response: {:?}", response));
        };
        let mut downloaded_packages = HashMap::new();
        for entry in entries {
            // ignore non-package dirs
            let Ok(package_id) = entry.path[1..].parse::<PackageId>() else { continue };
            if entry.file_type == vfs::FileType::Directory {
                let zip_file = vfs::File {
                    path: format!("/{}/pkg/{}.zip", package_id, package_id),
                };
                let Ok(zip_file_bytes) = zip_file.read() else { continue };
                // generate entry from this data
                // for the version hash, take the SHA-256 hash of the zip file
                let our_version = generate_version_hash(&zip_file_bytes);
                // the user will need to turn mirroring and auto-update back on if they
                // have to reset the state of their app store for some reason. the apps
                // themselves will remain on disk unless explicitly deleted.
                let package_state = PackageState {
                    mirrored_from: None,
                    our_version,
                    mirroring: false,
                    auto_update: false,
                    metadata: None,
                };
                downloaded_packages.insert(package_id, package_state);
            }
        }
        self.downloaded_packages = downloaded_packages;
        crate::set_state(&bincode::serialize(self)?);
        Ok(())
    }

    /// saves state
    pub fn add_downloaded_package(
        &mut self,
        zip_bytes: Vec<u8>,
        package_id: &PackageId,
        package_state: PackageState,
    ) -> anyhow::Result<()> {
        let drive_name = format!("/{package_id}/pkg");
        let blob = LazyLoadBlob {
            mime: Some("application/zip".to_string()),
            bytes: zip_bytes,
        };

        // create a new drive for this package in VFS
        // this is possible because we have root access
        Request::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: drive_name.clone(),
                action: vfs::VfsAction::CreateDrive,
            })?)
            .send_and_await_response(5)??;

        // convert the zip to a new package drive
        let response = Request::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: drive_name.clone(),
                action: vfs::VfsAction::AddZip,
            })?)
            .blob(blob.clone())
            .send_and_await_response(5)??;
        let vfs::VfsResponse::Ok = serde_json::from_slice::<vfs::VfsResponse>(response.body())? else {
            return Err(anyhow::anyhow!(
                "cannot add NewPackage: do not have capability to access vfs"
            ));
        };

        // save the zip file itself in VFS for sharing with other nodes
        // call it <package_id>.zip
        let zip_path = format!("{}/{}.zip", drive_name, package_id);
        Request::to(("our", "vfs", "distro", "sys"))
            // .inherit(true) is this needed?
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: zip_path,
                action: vfs::VfsAction::Write,
            })?)
            .blob(blob)
            .send_and_await_response(5)??;

        self.downloaded_packages
            .insert(package_id.clone(), package_state);
        crate::set_state(&bincode::serialize(self)?);
        Ok(())
    }

    /// saves state
    pub fn uninstall(&mut self, package_id: &PackageId) -> anyhow::Result<()> {
        let drive_path = format!("/{package_id}/pkg");
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
            .send_and_await_response(5)??;

        // finally, remove from downloaded packages
        self.downloaded_packages.remove(package_id);
        // save state
        crate::set_state(&bincode::serialize(self)?);
        Ok(())
    }

    /// only saves state if last_saved_block is more than 1000 blocks behind
    pub fn ingest_listings_contract_event(&mut self, log: Log) -> anyhow::Result<()> {
        let block_number: u64 = log
            .block_number
            .ok_or(anyhow::anyhow!("app store: got log with no block number"))?
            .try_into()?;

        // let package_hash: alloy_primitives::U256 = log.topics[1].into();
        // let package_hash = package_hash.to_string();

        match log.topics[0] {
            AppRegistered::SIGNATURE_HASH => {
                let (package_hash, package_name, package_publisher, metadata_hash) =
                    AppRegistered::abi_decode_data(&log.data, true)?;
                // TODO
                if generate_package_hash(&package_name, &package_publisher)
                    != package_hash.to_string()
                {
                    return Err(anyhow::anyhow!(
                        "app store: got log with mismatched package hash"
                    ));
                }
            }
            AppUnlisted::SIGNATURE_HASH => {
                // TODO
            }
            AppMetadataUpdated::SIGNATURE_HASH => {
                // TODO
            }
            Transfer::SIGNATURE_HASH => {
                // TODO
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn generate_package_hash(name: &str, publisher: &str) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update([name, publisher].concat());
    format!("{:x}", hasher.finalize())
}

pub fn generate_version_hash(zip_bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(zip_bytes);
    format!("{:x}", hasher.finalize())
}
