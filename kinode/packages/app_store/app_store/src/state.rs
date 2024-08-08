use crate::{utils, VFS_TIMEOUT};
use kinode_process_lib::{kimap, println, vfs, PackageId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

//
// main:app_store types
//

#[derive(Debug, Serialize, Deserialize)]
pub enum AppStoreLogError {
    NoBlockNumber,
    GetNameError,
    DecodeLogError(kimap::DecodeLogError),
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
            AppStoreLogError::DecodeLogError(e) => write!(f, "error decoding log data: {e:?}"),
            AppStoreLogError::PackageHashMismatch => write!(f, "mismatched package hash"),
            AppStoreLogError::InvalidPublisherName => write!(f, "invalid publisher name"),
            AppStoreLogError::MetadataNotFound => write!(f, "metadata not found"),
            AppStoreLogError::MetadataHashMismatch => write!(f, "metadata hash mismatch"),
            AppStoreLogError::PublisherNameMismatch => write!(f, "publisher name mismatch"),
        }
    }
}

impl std::error::Error for AppStoreLogError {}

/// state of an individual package we have downloaded
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageState {
    /// the version of the package we have installed
    pub our_version_hash: String,
    pub verified: bool,
    pub caps_approved: bool,
    /// the hash of the manifest file, which is used to determine whether package
    /// capabilities have changed. if they have changed, auto-install must fail
    /// and the user must approve the new capabilities.
    pub manifest_hash: Option<String>,
}

/// this process's saved state
pub struct State {
    /// packages we have installed
    pub packages: HashMap<PackageId, PackageState>,
    /// the APIs we have
    pub installed_apis: HashSet<PackageId>,
    // requested maybe too.
}

impl State {
    /// To load state, we populate the downloaded_packages map
    /// with all packages parseable from our filesystem.
    pub fn load() -> anyhow::Result<Self> {
        let mut state = State {
            packages: HashMap::new(),
            installed_apis: HashSet::new(),
        };
        state.populate_packages_from_filesystem()?;
        Ok(state)
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

        if let Ok(extracted) = utils::extract_api(package_id) {
            if extracted {
                self.installed_apis.insert(package_id.to_owned());
            }
        }
        Ok(())
    }

    // /// returns True if the package was found and updated, False otherwise
    // pub fn update_downloaded_package(
    //     &mut self,
    //     package_id: &PackageId,
    //     fn_: impl FnOnce(&mut PackageState),
    // ) -> bool {
    //     let res = self
    //         .packages
    //         .get_mut(package_id)
    //         .map(|listing| true)
    //         .unwrap_or(false);
    //     res
    // }

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

            self.packages.insert(
                package_id.clone(),
                PackageState {
                    our_version_hash,
                    verified: true,       // implicitly verified (TODO re-evaluate)
                    caps_approved: false, // must re-approve if you want to do something ??
                    manifest_hash: Some(utils::keccak_256_hash(&manifest_bytes)),
                },
            );

            if let Ok(Ok(_)) =
                utils::vfs_request(format!("/{package_id}/pkg/api"), vfs::VfsAction::Metadata)
                    .send_and_await_response(VFS_TIMEOUT)
            {
                self.installed_apis.insert(package_id);
            }
        }
        Ok(())
    }

    // TODO: re-evaluate
    pub fn uninstall(&mut self, package_id: &PackageId) -> anyhow::Result<()> {
        utils::uninstall(package_id)?;
        let Some(state) = self.packages.get_mut(package_id) else {
            return Err(anyhow::anyhow!("package not found"));
        };
        println!("uninstalled {package_id}");
        Ok(())
    }
}
