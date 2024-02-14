use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::eth::Log;
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::{println, *};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

sol! {
    event AppRegistered(
        uint256 indexed package,
        string packageName,
        bytes publisherName,
        string metadataUrl,
        bytes32 metadataHash
    );
    event AppMetadataUpdated(
        uint256 indexed package,
        string metadataUrl,
        bytes32 metadataHash
    );
    event Transfer(
        address indexed from,
        address indexed to,
        uint256 indexed tokenId
    );
}

/// from kns_indexer:sys
#[derive(Debug, Serialize, Deserialize)]
pub enum IndexerRequests {
    /// return the human readable name for a namehash
    /// returns an Option<String>
    NamehashToName { hash: String, block: u64 },
    /// return the most recent on-chain routing information for a node name.
    /// returns an Option<KnsUpdate>
    NodeInfo { name: String, block: u64 },
}

//
// app store types
//

pub type PackageHash = String;

/// listing information derived from metadata hash in listing event
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PackageListing {
    pub owner: String, // eth address
    pub name: String,
    pub publisher: NodeId,
    pub metadata_hash: String,
    pub metadata: Option<OnchainPackageMetadata>,
}

/// metadata derived from metadata hash in listing event
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainPackageMetadata {
    pub name: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub version: Option<String>,
    pub license: Option<String>,
    pub website: Option<String>,
    pub screenshots: Option<Vec<String>>,
    pub mirrors: Option<Vec<NodeId>>,
    pub versions: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestedPackage {
    pub from: NodeId,
    pub mirror: bool,
    pub auto_update: bool,
    // if none, we're requesting the latest version onchain
    pub desired_version_hash: Option<String>,
}

/// state of an individual package we have downloaded
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageState {
    /// the node we last downloaded the package from
    /// this is "us" if we don't know the source (usually cause it's a local install)
    pub mirrored_from: Option<NodeId>,
    /// the version of the package we have downloaded
    pub our_version: String,
    pub installed: bool,
    pub caps_approved: bool,
    pub manifest_hash: Option<String>,
    /// are we serving this package to others?
    pub mirroring: bool,
    /// if we get a listing data update, will we try to download it?
    pub auto_update: bool,
    pub metadata: Option<OnchainPackageMetadata>,
}

/// this process's saved state
#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    /// the address of the contract we are using to read package listings
    pub contract_address: String,
    /// the last block at which we saved the state of the listings to disk.
    /// we don't want to save the state every time we get a new listing,
    /// so we only save it every so often and then mark the block at which
    /// that last occurred here. when we boot, we can read logs starting
    /// from this block and rebuild latest state.
    pub last_saved_block: u64,
    /// we keep the full state of the package manager here, calculated from
    /// the listings contract logs. in the future, we'll offload this and
    /// only track a certain number of packages...
    pub package_hashes: HashMap<PackageId, PackageHash>, // TODO migrate to sqlite db
    pub listed_packages: HashMap<PackageHash, PackageListing>, // TODO migrate to sqlite db
    /// we keep the full state of the packages we have downloaded here.
    /// in order to keep this synchronized with our filesystem, we will
    /// ingest apps on disk if we have to rebuild our state. this is also
    /// updated every time we download, create, or uninstall a package.
    pub downloaded_packages: HashMap<PackageId, PackageState>, // TODO migrate to sqlite db
}

impl State {
    /// To create a new state, we populate the downloaded_packages map
    /// with all packages parseable from our filesystem.
    pub fn new(contract_address: String) -> anyhow::Result<Self> {
        crate::print_to_terminal(1, "app store: producing new state");
        let mut state = State {
            contract_address,
            last_saved_block: 1,
            package_hashes: HashMap::new(),
            listed_packages: HashMap::new(),
            downloaded_packages: HashMap::new(),
        };
        crate::print_to_terminal(
            1,
            &format!("populate: {:?}", state.populate_packages_from_filesystem()),
        );
        Ok(state)
    }

    pub fn get_listing(&self, package_id: &PackageId) -> Option<&PackageListing> {
        self.listed_packages
            .get(self.package_hashes.get(package_id)?)
    }

    fn get_listing_with_hash_mut(
        &mut self,
        package_hash: &PackageHash,
    ) -> Option<&mut PackageListing> {
        self.listed_packages.get_mut(package_hash)
    }

    /// Done in response to any new onchain listing update other than 'delete'
    fn insert_listing(&mut self, package_hash: PackageHash, listing: PackageListing) {
        self.package_hashes.insert(
            PackageId::new(&listing.name, &listing.publisher),
            package_hash.clone(),
        );
        self.listed_packages.insert(package_hash, listing);
    }

    /// Done in response to an onchain listing update of 'delete'
    fn delete_listing(&mut self, package_hash: &PackageHash) {
        if let Some(old) = self.listed_packages.remove(package_hash) {
            self.package_hashes
                .remove(&PackageId::new(&old.name, &old.publisher));
        }
    }

    pub fn get_downloaded_package(&self, package_id: &PackageId) -> Option<PackageState> {
        self.downloaded_packages.get(package_id).cloned()
    }

    pub fn add_downloaded_package(
        &mut self,
        package_id: &PackageId,
        mut package_state: PackageState,
        package_bytes: Option<Vec<u8>>,
    ) -> anyhow::Result<()> {
        if let Some(package_bytes) = package_bytes {
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
                .send_and_await_response(5)??;

            // convert the zip to a new package drive
            let response = Request::to(("our", "vfs", "distro", "sys"))
                .body(serde_json::to_vec(&vfs::VfsRequest {
                    path: drive_name.clone(),
                    action: vfs::VfsAction::AddZip,
                })?)
                .blob(blob.clone())
                .send_and_await_response(5)??;
            let vfs::VfsResponse::Ok = serde_json::from_slice::<vfs::VfsResponse>(response.body())?
            else {
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
                .send_and_await_response(5)??;

            let manifest_file = vfs::File {
                path: format!("/{}/pkg/manifest.json", package_id),
            };
            let manifest_bytes = manifest_file.read()?;
            let manifest_hash = generate_metadata_hash(&manifest_bytes);
            package_state.manifest_hash = Some(manifest_hash);
        }
        self.downloaded_packages
            .insert(package_id.to_owned(), package_state);
        crate::set_state(&bincode::serialize(self)?);
        Ok(())
    }

    /// returns True if the package was found and updated, False otherwise
    pub fn update_downloaded_package(
        &mut self,
        package_id: &PackageId,
        fn_: impl FnOnce(&mut PackageState),
    ) -> bool {
        let res = self
            .downloaded_packages
            .get_mut(package_id)
            .map(|package_state| {
                fn_(package_state);
                true
            })
            .unwrap_or(false);
        crate::set_state(&bincode::serialize(self).unwrap());
        res
    }

    pub fn start_mirroring(&mut self, package_id: &PackageId) -> bool {
        self.update_downloaded_package(package_id, |package_state| {
            package_state.mirroring = true;
        })
    }

    pub fn stop_mirroring(&mut self, package_id: &PackageId) -> bool {
        self.update_downloaded_package(package_id, |package_state| {
            package_state.mirroring = false;
        })
    }

    pub fn start_auto_update(&mut self, package_id: &PackageId) -> bool {
        self.update_downloaded_package(package_id, |package_state| {
            package_state.auto_update = true;
        })
    }

    pub fn stop_auto_update(&mut self, package_id: &PackageId) -> bool {
        self.update_downloaded_package(package_id, |package_state| {
            package_state.auto_update = false;
        })
    }

    /// saves state
    pub fn populate_packages_from_filesystem(&mut self) -> anyhow::Result<()> {
        let Message::Response { body, .. } = Request::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: "/".to_string(),
                action: vfs::VfsAction::ReadDir,
            })?)
            .send_and_await_response(3)??
        else {
            return Err(anyhow::anyhow!("vfs: bad response"));
        };
        let response = serde_json::from_slice::<vfs::VfsResponse>(&body)?;
        crate::print_to_terminal(1, &format!("vfs response: {:?}", response));
        let vfs::VfsResponse::ReadDir(entries) = response else {
            return Err(anyhow::anyhow!("vfs: unexpected response: {:?}", response));
        };
        for entry in entries {
            crate::print_to_terminal(1, &format!("entry: {:?}", entry));
            // ignore non-package dirs
            let Ok(package_id) = entry.path.parse::<PackageId>() else {
                continue;
            };
            if entry.file_type == vfs::FileType::Directory {
                let zip_file = vfs::File {
                    path: format!("/{}/pkg/{}.zip", package_id, package_id),
                };
                let Ok(zip_file_bytes) = zip_file.read() else {
                    continue;
                };
                // generate entry from this data
                // for the version hash, take the SHA-256 hash of the zip file
                let our_version = generate_version_hash(&zip_file_bytes);
                let manifest_file = vfs::File {
                    path: format!("/{}/pkg/manifest.json", package_id),
                };
                let manifest_bytes = manifest_file.read()?;
                // the user will need to turn mirroring and auto-update back on if they
                // have to reset the state of their app store for some reason. the apps
                // themselves will remain on disk unless explicitly deleted.
                self.add_downloaded_package(
                    &package_id,
                    PackageState {
                        mirrored_from: None,
                        our_version,
                        installed: true,
                        caps_approved: true, // since it's already installed this must be true
                        manifest_hash: Some(generate_metadata_hash(&manifest_bytes)),
                        mirroring: false,
                        auto_update: false,
                        metadata: None,
                    },
                    None,
                )?
            }
        }
        Ok(())
    }

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
        crate::set_state(&bincode::serialize(self)?);

        println!("app store: uninstalled {package_id}");
        Ok(())
    }

    /// only saves state if last_saved_block is more than 1000 blocks behind
    pub fn ingest_listings_contract_event(
        &mut self,
        our: &Address,
        log: Log,
    ) -> anyhow::Result<()> {
        let block_number: u64 = log
            .block_number
            .ok_or(anyhow::anyhow!("app store: got log with no block number"))?
            .try_into()?;

        // let package_hash: alloy_primitives::U256 = log.topics[1].into();
        // let package_hash = package_hash.to_string();

        match log.topics[0] {
            AppRegistered::SIGNATURE_HASH => {
                let package_hash = log.topics[1];
                let (package_name, publisher_dnswire, metadata_url, metadata_hash) =
                    AppRegistered::abi_decode_data(&log.data, true)?;
                let package_hash = package_hash.to_string();
                let metadata_hash = metadata_hash.to_string();

                crate::print_to_terminal(
                    1,
                    &format!(
                        "app registered with publisher_dnswire {:?}, package_hash {}, package_name {}, metadata_url {}, metadata_hash {}",
                        publisher_dnswire, package_hash, package_name, metadata_url, metadata_hash
                    )
                );

                if generate_package_hash(&package_name, publisher_dnswire.as_slice())
                    != package_hash
                {
                    return Err(anyhow::anyhow!(
                        "app store: got log with mismatched package hash"
                    ));
                }

                let Ok(publisher_name) = dnswire_decode(publisher_dnswire.as_slice()) else {
                    return Err(anyhow::anyhow!(
                        "app store: got log with invalid publisher name"
                    ));
                };

                let metadata = fetch_metadata(&metadata_url, &metadata_hash).ok();

                let listing = match self.get_listing_with_hash_mut(&package_hash) {
                    Some(current_listing) => {
                        current_listing.name = package_name;
                        current_listing.publisher = publisher_name;
                        current_listing.metadata_hash = metadata_hash;
                        current_listing.metadata = metadata;
                        current_listing.clone()
                    }
                    None => PackageListing {
                        owner: "".to_string(),
                        name: package_name,
                        publisher: publisher_name,
                        metadata_hash,
                        metadata,
                    },
                };
                self.insert_listing(package_hash, listing);
            }
            AppMetadataUpdated::SIGNATURE_HASH => {
                let package_hash = log.topics[1].to_string();
                let (metadata_url, metadata_hash) =
                    AppMetadataUpdated::abi_decode_data(&log.data, false)?;
                let metadata_hash = metadata_hash.to_string();

                crate::print_to_terminal(
                    1,
                    &format!(
                        "app metadata updated with package_hash {}, metadata_url {}, metadata_hash {}",
                        package_hash, metadata_url, metadata_hash
                    )
                );

                let current_listing = self
                    .get_listing_with_hash_mut(&package_hash.to_string())
                    .ok_or(anyhow::anyhow!(
                        "app store: got log with no matching listing"
                    ))?;

                let metadata = match fetch_metadata(&metadata_url, &metadata_hash) {
                    Ok(metadata) => Some(metadata),
                    Err(e) => {
                        crate::print_to_terminal(
                            1,
                            &format!("app store: failed to fetch metadata: {e:?}"),
                        );
                        None
                    }
                };

                current_listing.metadata_hash = metadata_hash;
                current_listing.metadata = metadata;

                let package_id = PackageId::new(&current_listing.name, &current_listing.publisher);

                // if we have this app installed, and we have auto_update set to true,
                // we should try to download new version from the mirrored_from node
                // and install it if successful.
                if let Some(package_state) = self.downloaded_packages.get(&package_id) {
                    if package_state.auto_update {
                        if let Some(mirrored_from) = &package_state.mirrored_from {
                            crate::print_to_terminal(
                                1,
                                &format!(
                                    "app store: auto-updating package {package_id} from {mirrored_from}"
                                ),
                            );
                            Request::to(our)
                                .body(serde_json::to_vec(&LocalRequest::Download {
                                    package: package_id,
                                    download_from: mirrored_from.clone(),
                                    mirror: package_state.mirroring,
                                    auto_update: package_state.auto_update,
                                    desired_version_hash: None,
                                })?)
                                .send()?;
                        }
                    }
                }
            }
            Transfer::SIGNATURE_HASH => {
                let from = alloy_primitives::Address::from_word(log.topics[1]);
                let to = alloy_primitives::Address::from_word(log.topics[2]);
                let package_hash = log.topics[3].to_string();

                crate::print_to_terminal(
                    1,
                    &format!(
                        "handling transfer from {} to {} of pkghash {}",
                        from, to, package_hash
                    ),
                );

                if from == alloy_primitives::Address::ZERO {
                    crate::print_to_terminal(1, "transfer from 0 address: new app listed");
                    match self.get_listing_with_hash_mut(&package_hash) {
                        Some(current_listing) => {
                            current_listing.owner = to.to_string();
                        }
                        None => {
                            let listing = PackageListing {
                                owner: to.to_string(),
                                name: "".to_string(),
                                publisher: "".to_string(),
                                metadata_hash: "".to_string(),
                                metadata: None,
                            };
                            self.insert_listing(package_hash, listing);
                        }
                    }
                } else if to == alloy_primitives::Address::ZERO {
                    crate::print_to_terminal(1, "transfer to 0 address: deleting listing");
                    self.delete_listing(&package_hash);
                } else {
                    crate::print_to_terminal(1, "transferring listing");
                    let current_listing =
                        self.get_listing_with_hash_mut(&package_hash)
                            .ok_or(anyhow::anyhow!(
                                "app store: got log with no matching listing"
                            ))?;
                    current_listing.owner = to.to_string();
                }
            }
            _ => {}
        }
        if block_number > self.last_saved_block + 1000 {
            self.last_saved_block = block_number;
            crate::set_state(&bincode::serialize(self)?);
        }
        Ok(())
    }
}

/// take a DNSwire-formatted node ID from chain and convert it to a String
fn dnswire_decode(wire_format_bytes: &[u8]) -> Result<String, std::string::FromUtf8Error> {
    let mut i = 0;
    let mut result = Vec::new();

    while i < wire_format_bytes.len() {
        let len = wire_format_bytes[i] as usize;
        if len == 0 {
            break;
        }
        let end = i + len + 1;
        let mut span = wire_format_bytes[i + 1..end].to_vec();
        span.push('.' as u8);
        result.push(span);
        i = end;
    }

    let flat: Vec<_> = result.into_iter().flatten().collect();

    let name = String::from_utf8(flat)?;

    // Remove the trailing '.' if it exists (it should always exist)
    if name.ends_with('.') {
        Ok(name[0..name.len() - 1].to_string())
    } else {
        Ok(name)
    }
}

/// fetch metadata from metadata_url and verify it matches metadata_hash
fn fetch_metadata(
    metadata_url: &str,
    metadata_hash: &str,
) -> anyhow::Result<OnchainPackageMetadata> {
    let url = url::Url::parse(metadata_url)?;
    let _response = http::send_request_await_response(http::Method::GET, url, None, 5, vec![])?;
    let Some(body) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let hash = generate_metadata_hash(&body.bytes);
    if &hash == metadata_hash {
        Ok(serde_json::from_slice::<OnchainPackageMetadata>(
            &body.bytes,
        )?)
    } else {
        Err(anyhow::anyhow!(
            "metadata hash mismatch: got {hash}, expected {metadata_hash}"
        ))
    }
}

/// generate a Keccak-256 hash of the metadata bytes
fn generate_metadata_hash(metadata: &[u8]) -> String {
    use sha3::{Digest, Keccak256};
    let mut hasher = Keccak256::new();
    hasher.update(metadata);
    format!("0x{:x}", hasher.finalize())
}

/// generate a Keccak-256 hash of the package name and publisher (match onchain)
fn generate_package_hash(name: &str, publisher_dnswire: &[u8]) -> String {
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
