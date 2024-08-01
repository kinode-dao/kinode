use crate::{utils, DownloadRequest, LocalRequest};
use crate::{KIMAP_ADDRESS, VFS_TIMEOUT};
use alloy_sol_types::SolEvent;
use kinode_process_lib::kernel_types::Erc721Metadata;
use kinode_process_lib::{
    eth, kernel_types as kt, kimap, println, vfs, Address, NodeId, PackageId, Request,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

//
// app store types
//

#[derive(Debug, Serialize, Deserialize)]
pub enum AppStoreLogError {
    NoBlockNumber,
    GetNameError,
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
            AppStoreLogError::GetNameError => write!(f, "no corresponding name for namehash found"),
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

#[derive(Debug, Deserialize, Serialize)]
pub struct MirroringFile {
    pub mirroring_from: Option<NodeId>,
    pub mirroring: bool,
    pub auto_update: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MirrorCheckFile {
    pub node: NodeId,
    pub is_online: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RequestedPackage {
    pub from: NodeId,
    pub mirror: bool,
    pub auto_update: bool,
    // if none, we're requesting the latest version onchain
    pub desired_version_hash: Option<String>,
}

/// listing information derived from metadata hash in listing event
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PackageListing {
    pub tba: eth::Address,
    pub metadata_uri: String,
    pub metadata_hash: String,
    pub metadata: Option<kt::Erc721Metadata>,
    /// if we have downloaded the package, this is populated
    pub state: Option<PackageState>,
}

/// state of an individual package we have downloaded
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageState {
    /// the node we last downloaded the package from
    /// this is "us" if we don't know the source (usually cause it's a local install)
    pub mirrored_from: Option<NodeId>,
    /// the version of the package we have downloaded
    pub our_version_hash: String,
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
}

/// this process's saved state
pub struct State {
    /// our address, grabbed from init()
    pub our: Address,
    /// the kimap helper we are using
    pub kimap: kimap::Kimap,
    /// the last block at which we saved the state of the listings to disk.
    /// when we boot, we can read logs starting from this block and
    /// rebuild latest state.
    pub last_saved_block: u64,
    /// we keep the full state of the package manager here, calculated from
    /// the listings contract logs. in the future, we'll offload this and
    /// only track a certain number of packages...
    pub packages: HashMap<PackageId, PackageListing>,
    /// the APIs we have
    pub downloaded_apis: HashSet<PackageId>,
    /// the packages we have outstanding requests to download (not persisted)
    pub requested_packages: HashMap<PackageId, RequestedPackage>,
    /// the APIs we have outstanding requests to download (not persisted)
    pub requested_apis: HashMap<PackageId, RequestedPackage>,
}

#[derive(Deserialize)]
pub struct SerializedState {
    pub kimap: kimap::Kimap,
    pub last_saved_block: u64,
    pub packages: HashMap<PackageId, PackageListing>,
    pub downloaded_apis: HashSet<PackageId>,
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("State", 6)?;
        state.serialize_field("kimap", &self.kimap)?;
        state.serialize_field("last_saved_block", &self.last_saved_block)?;
        state.serialize_field("packages", &self.packages)?;
        state.serialize_field("downloaded_apis", &self.downloaded_apis)?;
        state.end()
    }
}

impl State {
    pub fn from_serialized(our: Address, s: SerializedState) -> Self {
        State {
            our,
            kimap: s.kimap,
            last_saved_block: s.last_saved_block,
            packages: s.packages,
            downloaded_apis: s.downloaded_apis,
            requested_packages: HashMap::new(),
            requested_apis: HashMap::new(),
        }
    }

    /// To create a new state, we populate the downloaded_packages map
    /// with all packages parseable from our filesystem.
    pub fn new(our: Address, provider: eth::Provider) -> anyhow::Result<Self> {
        let mut state = State {
            our,
            kimap: kimap::Kimap::new(provider, eth::Address::from_str(KIMAP_ADDRESS).unwrap()),
            last_saved_block: crate::KIMAP_FIRST_BLOCK,
            packages: HashMap::new(),
            downloaded_apis: HashSet::new(),
            requested_packages: HashMap::new(),
            requested_apis: HashMap::new(),
        };
        state.populate_packages_from_filesystem()?;
        Ok(state)
    }

    pub fn add_listing(&mut self, package_id: &PackageId, metadata: kt::Erc721Metadata) {
        self.packages.insert(
            package_id.clone(),
            PackageListing {
                tba: eth::Address::ZERO,
                metadata_uri: "".to_string(),
                metadata_hash: utils::sha_256_hash(&serde_json::to_vec(&metadata).unwrap()),
                metadata: Some(metadata),
                state: None,
            },
        );
    }

    /// if package_bytes is None, we already have the package downloaded
    /// in VFS and this is being called to rebuild our process state
    pub fn add_downloaded_package(
        &mut self,
        package_id: &PackageId,
        mut package_state: PackageState,
        package_zip_bytes: Option<Vec<u8>>,
    ) -> anyhow::Result<()> {
        let Some(listing) = self.packages.get_mut(package_id) else {
            return Err(anyhow::anyhow!("package not found"));
        };
        // if passed zip bytes, make drive
        if let Some(package_bytes) = package_zip_bytes {
            let manifest_hash = utils::create_package_drive(package_id, package_bytes)?;
            package_state.manifest_hash = Some(manifest_hash);
        }
        // persist mirroring status
        let mirroring_file = vfs::File {
            path: format!("/{package_id}/pkg/.mirroring"),
            timeout: 5,
        };
        mirroring_file.write(&serde_json::to_vec(&MirroringFile {
            mirroring_from: package_state.mirrored_from.clone(),
            mirroring: package_state.mirroring,
            auto_update: package_state.auto_update,
        })?)?;
        if utils::extract_api(package_id)? {
            self.downloaded_apis.insert(package_id.to_owned());
        }
        listing.state = Some(package_state);
        // kinode_process_lib::set_state(&serde_json::to_vec(self)?);
        Ok(())
    }

    /// returns True if the package was found and updated, False otherwise
    pub fn update_downloaded_package(
        &mut self,
        package_id: &PackageId,
        fn_: impl FnOnce(&mut PackageState),
    ) -> bool {
        let res = self
            .packages
            .get_mut(package_id)
            .map(|listing| {
                if let Some(package_state) = &mut listing.state {
                    fn_(package_state);
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        // kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
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
        // call VFS and ask for all directories in our root drive
        // (we have root VFS capability so this is allowed)
        // we will interpret any that are package dirs and ingest them
        let vfs::VfsResponse::ReadDir(entries) = serde_json::from_slice::<vfs::VfsResponse>(
            utils::vfs_request("/", vfs::VfsAction::ReadDir)
                .send_and_await_response(VFS_TIMEOUT)??
                .body(),
        )?
        else {
            return Err(anyhow::anyhow!("vfs: unexpected response to ReadDir"));
        };
        for entry in entries {
            // ignore non-dirs
            if entry.file_type != vfs::FileType::Directory {
                continue;
            }
            // ignore non-package dirs
            let Ok(package_id) = entry.path.parse::<PackageId>() else {
                continue;
            };
            // grab package .zip if it exists
            let zip_file = vfs::File {
                path: format!("/{package_id}/pkg/{package_id}.zip"),
                timeout: 5,
            };
            let Ok(zip_file_bytes) = zip_file.read() else {
                continue;
            };
            // generate entry from this data
            // for the version hash, take the SHA-256 hash of the zip file
            let our_version_hash = utils::sha_256_hash(&zip_file_bytes);
            let manifest_file = vfs::File {
                path: format!("/{package_id}/pkg/manifest.json"),
                timeout: 5,
            };
            let manifest_bytes = manifest_file.read()?;
            // get mirroring data if available
            let mirroring_file = vfs::File {
                path: format!("/{package_id}/pkg/.mirroring"),
                timeout: 5,
            };
            let mirroring_data = if let Ok(bytes) = mirroring_file.read() {
                serde_json::from_slice::<MirroringFile>(&bytes)?
            } else {
                MirroringFile {
                    mirroring_from: None,
                    mirroring: false,
                    auto_update: false,
                }
            };
            self.packages.insert(
                package_id.clone(),
                PackageListing {
                    tba: eth::Address::ZERO,
                    metadata_uri: "".to_string(),
                    metadata_hash: "".to_string(),
                    metadata: None,
                    state: None,
                },
            );
            self.add_downloaded_package(
                &package_id,
                PackageState {
                    mirrored_from: mirroring_data.mirroring_from,
                    our_version_hash,
                    installed: true,
                    verified: true,       // implicitly verified (TODO re-evaluate)
                    caps_approved: false, // must re-approve if you want to do something
                    manifest_hash: Some(utils::keccak_256_hash(&manifest_bytes)),
                    mirroring: mirroring_data.mirroring,
                    auto_update: mirroring_data.auto_update,
                },
                None,
            )?;

            if let Ok(Ok(_)) =
                utils::vfs_request(format!("/{package_id}/pkg/api"), vfs::VfsAction::Metadata)
                    .send_and_await_response(VFS_TIMEOUT)
            {
                self.downloaded_apis.insert(package_id);
            }
        }
        Ok(())
    }

    pub fn uninstall(&mut self, package_id: &PackageId) -> anyhow::Result<()> {
        utils::uninstall(package_id)?;
        let Some(listing) = self.packages.get_mut(package_id) else {
            return Err(anyhow::anyhow!("package not found"));
        };
        listing.state = None;
        // kinode_process_lib::set_state(&serde_json::to_vec(self)?);
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

        let note: kimap::Note =
            kimap::decode_note_log(&log).map_err(|_| AppStoreLogError::DecodeLogError)?;

        let package_id = note
            .parent_path
            .split_once('.')
            .ok_or(AppStoreLogError::InvalidPublisherName)
            .and_then(|(package, publisher)| {
                if package.is_empty() || publisher.is_empty() {
                    Err(AppStoreLogError::InvalidPublisherName)
                } else {
                    Ok(PackageId::new(&package, &publisher))
                }
            })?;

        // the app store exclusively looks for ~metadata-uri postings: if one is
        // observed, we then *query* for ~metadata-hash to verify the content
        // at the URI.
        //
        let metadata_uri = String::from_utf8_lossy(&note.data).to_string();

        // generate ~metadata-hash notehash
        let hash_note = format!("~metadata-hash.{}", note.parent_path);

        // owner can change which we don't track (yet?) so don't save, need to get when desired
        let (tba, _owner, data) = self.kimap.get(&hash_note).map_err(|e| {
            println!("Couldn't find {hash_note}: {e:?}");
            AppStoreLogError::MetadataHashMismatch
        })?;

        let Some(hash_note) = data else {
            return Err(AppStoreLogError::MetadataNotFound);
        };

        let metadata_hash = String::from_utf8_lossy(&hash_note).to_string();

        // fetch metadata from the URI (currently only handling HTTP(S) URLs!)
        // assert that the metadata hash matches the fetched data
        let metadata = if update_listings {
            Some(utils::fetch_metadata_from_url(
                &metadata_uri,
                &metadata_hash,
                30,
            )?)
        } else {
            None
        };

        match self.packages.entry(package_id) {
            std::collections::hash_map::Entry::Occupied(mut listing) => {
                let listing = listing.get_mut();
                listing.tba = tba;
                listing.metadata_uri = metadata_uri;
                listing.metadata_hash = metadata_hash;
                if update_listings {
                    listing.metadata = metadata;
                }
            }
            std::collections::hash_map::Entry::Vacant(listing) => {
                listing.insert(PackageListing {
                    tba,
                    metadata_uri,
                    metadata_hash,
                    metadata,
                    state: None,
                });
            }
        };
        self.last_saved_block = block_number;
        // if update_listings {
        //     kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
        // }
        Ok(())
    }

    /// iterate through all package listings and try to fetch metadata.
    /// this is done after ingesting a bunch of logs to remove fetches
    /// of stale metadata.
    pub fn update_listings(&mut self) {
        for (package_id, listing) in self.packages.iter_mut() {
            if let Ok(metadata) =
                utils::fetch_metadata_from_url(&listing.metadata_uri, &listing.metadata_hash, 30)
            {
                if let Some(package_state) = &listing.state {
                    auto_update(&self.our, package_id, &metadata, package_state);
                }
                listing.metadata = Some(metadata);
            }
        }
        // kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
    }
}

/// if we have this app installed, and we have auto_update set to true,
/// we should try to download new version from the mirrored_from node
/// and install it if successful.
fn auto_update(
    our: &Address,
    package_id: &PackageId,
    metadata: &Erc721Metadata,
    package_state: &PackageState,
) {
    if package_state.auto_update {
        let latest_version_hash = metadata
            .properties
            .code_hashes
            .get(&metadata.properties.current_version);
        if let Some(mirrored_from) = &package_state.mirrored_from
            && Some(&package_state.our_version_hash) != latest_version_hash
        {
            println!(
                "auto-updating package {package_id} from {} to {} using mirror {mirrored_from}",
                metadata
                    .properties
                    .code_hashes
                    .get(&package_state.our_version_hash)
                    .unwrap_or(&package_state.our_version_hash),
                metadata.properties.current_version,
            );
            Request::to(our)
                .body(
                    serde_json::to_vec(&LocalRequest::Download(DownloadRequest {
                        package_id: crate::kinode::process::main::PackageId::from_process_lib(
                            package_id.clone(),
                        ),
                        download_from: mirrored_from.clone(),
                        mirror: package_state.mirroring,
                        auto_update: package_state.auto_update,
                        desired_version_hash: None,
                    }))
                    .unwrap(),
                )
                .send()
                .unwrap();
        }
    }
}
