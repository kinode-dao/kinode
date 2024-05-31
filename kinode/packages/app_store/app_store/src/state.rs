use crate::utils;
use crate::VFS_TIMEOUT;
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::{
    eth, kernel_types as kt, net, println, vfs, Address, Message, NodeId, PackageId, Request,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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

//
// app store types
//

#[derive(Debug, Serialize, Deserialize)]
pub enum AppStoreLogError {
    NoBlockNumber,
    DecodeLogError,
    PackageHashMismatch,
    InvalidPublisherName,
    MetadataNotFound,
    MetadataHashMismatch,
    PublisherNameMismatch,
}

impl std::fmt::Display for AppStoreLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AppStoreLogError::NoBlockNumber => write!(f, "log with no block number"),
            AppStoreLogError::DecodeLogError => write!(f, "error decoding log data"),
            AppStoreLogError::PackageHashMismatch => write!(f, "mismatched package hash"),
            AppStoreLogError::InvalidPublisherName => write!(f, "invalid publisher name"),
            AppStoreLogError::MetadataNotFound => write!(f, "metadata not found"),
            AppStoreLogError::MetadataHashMismatch => write!(f, "metadata hash mismatch"),
            AppStoreLogError::PublisherNameMismatch => write!(f, "publisher name mismatch"),
        }
    }
}

impl std::error::Error for AppStoreLogError {}

pub type PackageHash = String;

/// listing information derived from metadata hash in listing event
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PackageListing {
    pub owner: String, // eth address
    pub name: String,
    pub publisher: NodeId,
    pub metadata_url: String,
    pub metadata_hash: String,
    pub metadata: Option<kt::Erc721Metadata>,
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
    pub verified: bool,
    pub caps_approved: bool,
    /// the hash of the manifest file, which is used to determine whether package
    /// capabilities have changed. if they have changed, auto-install must fail
    /// and the user must approve the new capabilities.
    pub manifest_hash: Option<String>,
    /// are we serving this package to others?
    pub mirroring: bool,
    /// if we get a listing data update, will we try to download it?
    pub auto_update: bool,
    pub metadata: Option<kt::Erc721Metadata>,
}

/// this process's saved state
pub struct State {
    /// our address, grabbed from init()
    pub our: Address,
    /// the eth provider we are using -- not persisted
    pub provider: eth::Provider,
    /// the address of the contract we are using to read package listings
    pub contract_address: String,
    /// the last block at which we saved the state of the listings to disk.
    /// when we boot, we can read logs starting from this block and
    /// rebuild latest state.
    pub last_saved_block: u64,
    pub package_hashes: HashMap<PackageId, PackageHash>,
    /// we keep the full state of the package manager here, calculated from
    /// the listings contract logs. in the future, we'll offload this and
    /// only track a certain number of packages...
    pub listed_packages: HashMap<PackageHash, PackageListing>,
    /// we keep the full state of the packages we have downloaded here.
    /// in order to keep this synchronized with our filesystem, we will
    /// ingest apps on disk if we have to rebuild our state. this is also
    /// updated every time we download, create, or uninstall a package.
    pub downloaded_packages: HashMap<PackageId, PackageState>,
    /// the APIs we have
    pub downloaded_apis: HashSet<PackageId>,
    /// the packages we have outstanding requests to download (not persisted)
    pub requested_packages: HashMap<PackageId, RequestedPackage>,
    /// the APIs we have outstanding requests to download (not persisted)
    pub requested_apis: HashMap<PackageId, RequestedPackage>,
}

#[derive(Deserialize)]
pub struct SerializedState {
    pub contract_address: String,
    pub last_saved_block: u64,
    pub package_hashes: HashMap<PackageId, PackageHash>,
    pub listed_packages: HashMap<PackageHash, PackageListing>,
    pub downloaded_packages: HashMap<PackageId, PackageState>,
    pub downloaded_apis: HashSet<PackageId>,
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("State", 6)?;
        state.serialize_field("contract_address", &self.contract_address)?;
        state.serialize_field("last_saved_block", &self.last_saved_block)?;
        state.serialize_field("package_hashes", &self.package_hashes)?;
        state.serialize_field("listed_packages", &self.listed_packages)?;
        state.serialize_field("downloaded_packages", &self.downloaded_packages)?;
        state.serialize_field("downloaded_apis", &self.downloaded_apis)?;
        state.end()
    }
}

impl State {
    pub fn from_serialized(our: Address, provider: eth::Provider, s: SerializedState) -> Self {
        State {
            our,
            provider,
            contract_address: s.contract_address,
            last_saved_block: s.last_saved_block,
            package_hashes: s.package_hashes,
            listed_packages: s.listed_packages,
            downloaded_packages: s.downloaded_packages,
            downloaded_apis: s.downloaded_apis,
            requested_packages: HashMap::new(),
            requested_apis: HashMap::new(),
        }
    }
    /// To create a new state, we populate the downloaded_packages map
    /// with all packages parseable from our filesystem.
    pub fn new(
        our: Address,
        provider: eth::Provider,
        contract_address: String,
    ) -> anyhow::Result<Self> {
        let mut state = State {
            our,
            provider,
            contract_address,
            last_saved_block: crate::CONTRACT_FIRST_BLOCK,
            package_hashes: HashMap::new(),
            listed_packages: HashMap::new(),
            downloaded_packages: HashMap::new(),
            downloaded_apis: HashSet::new(),
            requested_packages: HashMap::new(),
            requested_apis: HashMap::new(),
        };
        state.populate_packages_from_filesystem()?;
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
            let manifest_hash = utils::create_package_drive(package_id, package_bytes)?;
            package_state.manifest_hash = Some(manifest_hash);
        }
        if utils::extract_api(package_id)? {
            self.downloaded_apis.insert(package_id.to_owned());
        }
        self.downloaded_packages
            .insert(package_id.to_owned(), package_state);
        kinode_process_lib::set_state(&serde_json::to_vec(self)?);
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
        kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
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
            .send_and_await_response(VFS_TIMEOUT)??
        else {
            return Err(anyhow::anyhow!("vfs: bad response"));
        };
        let response = serde_json::from_slice::<vfs::VfsResponse>(&body)?;
        let vfs::VfsResponse::ReadDir(entries) = response else {
            return Err(anyhow::anyhow!("vfs: unexpected response: {:?}", response));
        };
        for entry in entries {
            // ignore non-package dirs
            let Ok(package_id) = entry.path.parse::<PackageId>() else {
                continue;
            };
            if entry.file_type == vfs::FileType::Directory {
                let zip_file = vfs::File {
                    path: format!("/{}/pkg/{}.zip", package_id, package_id),
                    timeout: 5,
                };
                let Ok(zip_file_bytes) = zip_file.read() else {
                    continue;
                };
                // generate entry from this data
                // for the version hash, take the SHA-256 hash of the zip file
                let our_version = utils::generate_version_hash(&zip_file_bytes);
                let manifest_file = vfs::File {
                    path: format!("/{}/pkg/manifest.json", package_id),
                    timeout: 5,
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
                        verified: true,      // implicity verified
                        caps_approved: true, // since it's already installed this must be true
                        manifest_hash: Some(utils::generate_metadata_hash(&manifest_bytes)),
                        mirroring: false,
                        auto_update: false,
                        metadata: None,
                    },
                    None,
                )?;

                if let Ok(Ok(_)) = Request::new()
                    .target(("our", "vfs", "distro", "sys"))
                    .body(
                        serde_json::to_vec(&vfs::VfsRequest {
                            path: format!("/{package_id}/pkg/api"),
                            action: vfs::VfsAction::Metadata,
                        })
                        .unwrap(),
                    )
                    .send_and_await_response(VFS_TIMEOUT)
                {
                    self.downloaded_apis.insert(package_id.to_owned());
                }
            }
        }
        Ok(())
    }

    pub fn uninstall(&mut self, package_id: &PackageId) -> anyhow::Result<()> {
        utils::uninstall(package_id)?;
        self.downloaded_packages.remove(package_id);
        kinode_process_lib::set_state(&serde_json::to_vec(self)?);
        println!("uninstalled {package_id}");
        Ok(())
    }

    /// saves state
    ///
    /// only saves the onchain data in our package listings --
    /// in order to fetch metadata and trigger auto-update for all packages,
    /// call [`State::update_listings`], or call this with `true` as the third argument.
    pub fn ingest_contract_event(
        &mut self,
        log: eth::Log,
        update_listings: bool,
    ) -> Result<(), AppStoreLogError> {
        let block_number: u64 = log.block_number.ok_or(AppStoreLogError::NoBlockNumber)?;

        match log.topics()[0] {
            AppRegistered::SIGNATURE_HASH => {
                let app = AppRegistered::decode_log_data(log.data(), false)
                    .map_err(|_| AppStoreLogError::DecodeLogError)?;
                let package_name = app.packageName;
                let publisher_dnswire = app.publisherName;
                let metadata_url = app.metadataUrl;
                let metadata_hash = app.metadataHash;

                let package_hash = log.topics()[1].to_string();
                let metadata_hash = metadata_hash.to_string();

                kinode_process_lib::print_to_terminal(
                    1,
                    &format!("new package {package_name} registered onchain"),
                );

                if utils::generate_package_hash(&package_name, &publisher_dnswire) != package_hash {
                    return Err(AppStoreLogError::PackageHashMismatch);
                }

                let Ok(publisher_name) = net::dnswire_decode(&publisher_dnswire) else {
                    return Err(AppStoreLogError::InvalidPublisherName);
                };

                let metadata = if update_listings {
                    let metadata =
                        utils::fetch_metadata_from_url(&metadata_url, &metadata_hash, 5)?;
                    if metadata.properties.publisher != publisher_name {
                        return Err(AppStoreLogError::PublisherNameMismatch);
                    }
                    Some(metadata)
                } else {
                    None
                };

                self.package_hashes.insert(
                    PackageId::new(&package_name, &publisher_name),
                    package_hash.clone(),
                );

                match self.listed_packages.entry(package_hash) {
                    std::collections::hash_map::Entry::Occupied(mut listing) => {
                        let listing = listing.get_mut();
                        listing.name = package_name;
                        listing.publisher = publisher_name;
                        listing.metadata_url = metadata_url;
                        listing.metadata_hash = metadata_hash;
                        listing.metadata = metadata;
                    }
                    std::collections::hash_map::Entry::Vacant(listing) => {
                        listing.insert(PackageListing {
                            owner: "".to_string(),
                            name: package_name,
                            publisher: publisher_name,
                            metadata_url,
                            metadata_hash,
                            metadata,
                        });
                    }
                };

                // let api_hash = None; // TODO
                // let api_download_request_result = start_download(
                // state,
                // PackageId::new(&package_name, &publisher_name),
                // &publisher_name,
                // api_hash,
                // true,
                // );
                // match api_download_request_result {
                // DownloadResponse::Failure => println!("failed to get API for {package_name}"),
                // _ => {}
                // }
            }
            AppMetadataUpdated::SIGNATURE_HASH => {
                let upd = AppMetadataUpdated::decode_log_data(log.data(), false)
                    .map_err(|_| AppStoreLogError::DecodeLogError)?;
                let metadata_url = upd.metadataUrl;
                let metadata_hash = upd.metadataHash;

                let package_hash = log.topics()[1].to_string();
                let metadata_hash = metadata_hash.to_string();

                let Some(current_listing) =
                    self.get_listing_with_hash_mut(&package_hash.to_string())
                else {
                    // package not found, so we can't update it
                    // this will never happen if we're ingesting logs in order
                    return Ok(());
                };

                let metadata = if update_listings {
                    Some(utils::fetch_metadata_from_url(
                        &metadata_url,
                        &metadata_hash,
                        5,
                    )?)
                } else {
                    None
                };

                current_listing.metadata_url = metadata_url;
                current_listing.metadata_hash = metadata_hash;
                current_listing.metadata = metadata;

                // if we have this app installed, and we have auto_update set to true,
                // we should try to download new version from the mirrored_from node
                // and install it if successful.
                // let package_id = PackageId::new(&current_listing.name, &current_listing.publisher);
                // if let Some(package_state) = self.downloaded_packages.get(&package_id) {
                //     if package_state.auto_update {
                //         if let Some(mirrored_from) = &package_state.mirrored_from {
                //             kinode_process_lib::print_to_terminal(
                //                 1,
                //                 &format!("auto-updating package {package_id} from {mirrored_from}"),
                //             );
                //             Request::to(&self.our)
                //                 .body(serde_json::to_vec(&LocalRequest::Download(
                //                     DownloadRequest {
                //                         package_id: crate::kinode::process::main::PackageId::from_process_lib(
                //                             package_id,
                //                         ),
                //                         download_from: mirrored_from.clone(),
                //                         mirror: package_state.mirroring,
                //                         auto_update: package_state.auto_update,
                //                         desired_version_hash: None,
                //                     },
                //                 )).unwrap())
                //                 .send().unwrap();
                //         }
                //     }
                // }
            }
            Transfer::SIGNATURE_HASH => {
                let from = alloy_primitives::Address::from_word(log.topics()[1]);
                let to = alloy_primitives::Address::from_word(log.topics()[2]);
                let package_hash = log.topics()[3].to_string();

                if from == alloy_primitives::Address::ZERO {
                    // this is a new package, set the owner
                    match self.listed_packages.entry(package_hash) {
                        std::collections::hash_map::Entry::Occupied(mut listing) => {
                            let listing = listing.get_mut();
                            listing.owner = to.to_string();
                        }
                        std::collections::hash_map::Entry::Vacant(listing) => {
                            listing.insert(PackageListing {
                                owner: to.to_string(),
                                name: "".to_string(),
                                publisher: "".to_string(),
                                metadata_url: "".to_string(),
                                metadata_hash: "".to_string(),
                                metadata: None,
                            });
                        }
                    };
                } else if to == alloy_primitives::Address::ZERO {
                    // this is a package deletion
                    if let Some(old) = self.listed_packages.remove(&package_hash) {
                        self.package_hashes
                            .remove(&PackageId::new(&old.name, &old.publisher));
                    }
                } else {
                    let Some(listing) = self.get_listing_with_hash_mut(&package_hash) else {
                        // package not found, so we can't update it
                        // this will never happen if we're ingesting logs in order
                        return Ok(());
                    };
                    listing.owner = to.to_string();
                }
            }
            _ => {}
        }
        self.last_saved_block = block_number;
        if update_listings {
            kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
        }
        Ok(())
    }

    /// iterate through all package listings and try to fetch metadata.
    /// this is done after ingesting a bunch of logs to remove fetches
    /// of stale metadata.
    pub fn update_listings(&mut self) {
        for (_package_hash, listing) in self.listed_packages.iter_mut() {
            if listing.metadata.is_none() {
                if let Ok(metadata) =
                    utils::fetch_metadata_from_url(&listing.metadata_url, &listing.metadata_hash, 5)
                {
                    listing.metadata = Some(metadata);
                }
            }
        }
        kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
    }
}
