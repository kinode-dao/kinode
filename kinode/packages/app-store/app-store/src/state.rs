use crate::{utils, VFS_TIMEOUT};
use kinode_process_lib::{kimap, vfs, PackageId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

//
// main:app-store types
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MirrorCheck {
    pub node: String,
    pub is_online: bool,
    pub error: Option<String>,
}

/// state of an individual package we have downloaded
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageState {
    /// the version of the package we have installed
    pub our_version_hash: String,
    pub verified: bool,
    pub caps_approved: bool,
    /// the hash of the manifest, which is used to determine whether package
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
            let manifest_hash = utils::keccak_256_hash(&manifest_bytes);
            self.packages.insert(
                package_id.clone(),
                PackageState {
                    our_version_hash,
                    verified: true,       // implicitly verified (TODO re-evaluate)
                    caps_approved: false, // must re-approve if you want to do something ??
                    manifest_hash: Some(manifest_hash),
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
}
