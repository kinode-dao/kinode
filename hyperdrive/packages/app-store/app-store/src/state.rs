use crate::{hyperware::process::downloads::DownloadError, utils, VFS_TIMEOUT};
use hyperware_process_lib::{get_state, hypermap, set_state, vfs, PackageId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

//
// main:app-store types
//

#[derive(Debug, Serialize, Deserialize)]
pub enum AppStoreLogError {
    NoBlockNumber,
    GetNameError,
    DecodeLogError(hypermap::DecodeLogError),
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

// this seems cleaner to me right now with pending_update_hash, but given how we serialize
// the state to disk right now, with installed_apis and packages being populated directly
// from the filesystem, not sure I'd like to serialize the whole of this state (maybe separate out the pending one?)
// another option would be to have the download_api recheck the manifest hash? but not sure...
// arbitrary complexity here.

// alternative is main loop doing this, storing it.

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Updates {
    #[serde(with = "package_id_map")]
    pub package_updates: HashMap<PackageId, HashMap<String, UpdateInfo>>, // package id -> version_hash -> update info
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            package_updates: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub errors: Vec<(String, DownloadError)>, // errors collected by downloads process
    pub pending_manifest_hash: Option<String>, // pending manifest hash that differed from the installed one
}

impl Updates {
    pub fn load() -> Self {
        let bytes = get_state();

        if let Some(bytes) = bytes {
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        set_state(&bytes);
    }
}

// note: serde_json doesn't support non-string keys when serializing maps, so
// we have to use a custom simple serializer.
mod package_id_map {
    use super::*;
    use std::{collections::HashMap, str::FromStr};

    pub fn serialize<S>(
        map: &HashMap<PackageId, HashMap<String, UpdateInfo>>,
        s: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map_ser = s.serialize_map(Some(map.len()))?;
        for (k, v) in map {
            map_ser.serialize_entry(&k.to_string(), v)?;
        }
        map_ser.end()
    }

    pub fn deserialize<'de, D>(
        d: D,
    ) -> Result<HashMap<PackageId, HashMap<String, UpdateInfo>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string_map = HashMap::<String, HashMap<String, UpdateInfo>>::deserialize(d)?;
        Ok(string_map
            .into_iter()
            .filter_map(|(k, v)| PackageId::from_str(&k).ok().map(|pid| (pid, v)))
            .collect())
    }
}
