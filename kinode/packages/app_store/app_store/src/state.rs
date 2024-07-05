use crate::{utils, DownloadRequest, LocalRequest};
use crate::{KIMAP_ADDRESS, VFS_TIMEOUT};
use alloy_sol_types::{sol, SolEvent};
use kinode_process_lib::kernel_types::Erc721Metadata;
use kinode_process_lib::{
    eth, kernel_types as kt,
    net::{get_name, namehash},
    println, vfs, Address, Message, NodeId, PackageId, Request,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

sol! {
    event Note(bytes32 indexed nodehash, bytes32 indexed notehash, bytes indexed labelhash, bytes note, bytes data);
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
    pub owner: String, // eth address,
    pub name: String,
    pub publisher: NodeId, // this should be moved to metadata...
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
            last_saved_block: crate::KIMAP_FIRST_BLOCK,
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
            .downloaded_packages
            .get_mut(package_id)
            .map(|package_state| {
                fn_(package_state);
                true
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
                        verified: true,       // implicitly verified (TODO re-evaluate)
                        caps_approved: false, // must re-approve if you want to do something
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

        // basic plan...
        // when we get either metadata-uri or metadata-hash, we fetch the other one and see if they match.
        // if they do, we update the metadata for the package.
        // note: if either of hash/uri doens't match//errors, we probably shouldn't throw errors except for in verbose mode.

        // TEMP WAIT while we solve kimap_indexer getting race condition
        std::thread::sleep(std::time::Duration::from_millis(100));
        match log.topics()[0] {
            Note::SIGNATURE_HASH => {
                let note = Note::decode_log_data(log.data(), false)
                    .map_err(|_| AppStoreLogError::DecodeLogError)?;

                // get package_name from the api (add to process_lib)!
                let name = get_name(&note.nodehash.to_string(), None).map_err(|e| {
                    println!("Error decoding name: {:?}", e);
                    AppStoreLogError::DecodeLogError
                })?;

                let note_str = String::from_utf8_lossy(&note.note).to_string();

                let kimap = self
                    .provider
                    .kimap_with_address(eth::Address::from_str(KIMAP_ADDRESS).unwrap());
                // println!("got note {note_str} for {name}");
                // let notehash = note.notehash.to_string();
                // let full_name = format!("{note_str}.{name}");

                match note_str.as_str() {
                    "~metadata-uri" => {
                        let metadata_url = String::from_utf8_lossy(&note.data).to_string();
                        // generate ~metadata-hash notehash
                        let meta_note_name = format!("~metadata-hash.{name}");
                        let package_hash_note = namehash(&meta_note_name);
                        let (_tba, _owner, data) = kimap.get(&package_hash_note).map_err(|e| {
                            println!("Error getting metadata hash: {:?}", e);
                            AppStoreLogError::DecodeLogError
                        })?;

                        if let Some(hash_note) = data {
                            let metadata_hash = String::from_utf8_lossy(&hash_note).to_string();
                            let metadata =
                                utils::fetch_metadata_from_url(&metadata_url, &metadata_hash, 5)?;

                            // if this fails and doesn't check out, do nothing

                            let (package_name, publisher_name) = name
                                .split_once('.')
                                .ok_or(AppStoreLogError::InvalidPublisherName)
                                .and_then(|(package, publisher)| {
                                    if package.is_empty() || publisher.is_empty() {
                                        Err(AppStoreLogError::InvalidPublisherName)
                                    } else {
                                        Ok((package.to_string(), publisher.to_string()))
                                    }
                                })?;
                            println!(
                                "pkg_name and publisher_name: {package_name} {publisher_name}"
                            );
                            // do we need package hashes anymore? seems kinda unnecessary, use nodehashes instead?
                            // not removing for now for state compatibility
                            let package_hash = utils::generate_package_hash(
                                &package_name,
                                publisher_name.as_bytes(),
                            );

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
                                    listing.metadata = Some(metadata);
                                }
                                std::collections::hash_map::Entry::Vacant(listing) => {
                                    listing.insert(PackageListing {
                                        owner: "".to_string(),
                                        name: package_name,
                                        publisher: publisher_name,
                                        metadata_url,
                                        metadata_hash,
                                        metadata: Some(metadata),
                                    });
                                }
                            };
                        }
                    }
                    "~metadata-hash" => {
                        let metadata_hash = String::from_utf8_lossy(&note.data).to_string();
                        // generate ~metadata-uri notehash
                        let meta_note_name = format!("~metadata-uri.{name}");
                        let package_uri_note = namehash(&meta_note_name);
                        let (_tba, _owner, data) = kimap.get(&package_uri_note).map_err(|e| {
                            println!("Error getting metadata uri: {:?}", e);
                            AppStoreLogError::DecodeLogError
                        })?;

                        if let Some(uri_note) = data {
                            let metadata_url = String::from_utf8_lossy(&uri_note).to_string();
                            let metadata =
                                utils::fetch_metadata_from_url(&metadata_url, &metadata_hash, 5)?;

                            let (package_name, publisher_name) = name
                                .split_once('.')
                                .ok_or(AppStoreLogError::InvalidPublisherName)
                                .and_then(|(package, publisher)| {
                                    if package.is_empty() || publisher.is_empty() {
                                        Err(AppStoreLogError::InvalidPublisherName)
                                    } else {
                                        Ok((package.to_string(), publisher.to_string()))
                                    }
                                })?;
                            println!(
                                "pkg_name and publisher_name: {package_name} {publisher_name}"
                            );
                            // do we need package hashes anymore? seems kinda unnecessary, use nodehashes instead?
                            // not removing for now for state compatibility
                            let package_hash = utils::generate_package_hash(
                                &package_name,
                                publisher_name.as_bytes(),
                            );

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
                                    listing.metadata = Some(metadata);
                                }
                                std::collections::hash_map::Entry::Vacant(listing) => {
                                    listing.insert(PackageListing {
                                        owner: "".to_string(),
                                        name: package_name,
                                        publisher: publisher_name,
                                        metadata_url,
                                        metadata_hash,
                                        metadata: Some(metadata),
                                    });
                                }
                            };
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.last_saved_block = block_number;
        if update_listings {
            // kinode_process_lib::set_state(&serde_json::to_vec(self).unwrap());
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
                    let package_id = PackageId::new(&listing.name, &listing.publisher);
                    if let Some(package_state) = self.downloaded_packages.get(&package_id) {
                        auto_update(&self.our, package_id, &metadata, &package_state);
                    }
                    listing.metadata = Some(metadata);
                }
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
    package_id: PackageId,
    metadata: &Erc721Metadata,
    package_state: &PackageState,
) {
    if package_state.auto_update {
        let latest_version_hash = metadata
            .properties
            .code_hashes
            .get(&metadata.properties.current_version);
        if let Some(mirrored_from) = &package_state.mirrored_from
            && Some(&package_state.our_version) != latest_version_hash
        {
            println!(
                "auto-updating package {package_id} from {} to {} using mirror {mirrored_from}",
                metadata
                    .properties
                    .code_hashes
                    .get(&package_state.our_version)
                    .unwrap_or(&package_state.our_version),
                metadata.properties.current_version,
            );
            Request::to(our)
                .body(
                    serde_json::to_vec(&LocalRequest::Download(DownloadRequest {
                        package_id: crate::kinode::process::main::PackageId::from_process_lib(
                            package_id,
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
