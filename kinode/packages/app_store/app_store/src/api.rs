use kinode_process_lib::*;
use serde::{Deserialize, Serialize};

//
// app store API
//

/// Remote requests, those sent between instantiations of this process
/// on different nodes, take this form. Will add more here in the future
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteRequest {
    /// Request a package from another node who we expect to
    /// be mirroring it. If the remote node is mirroring the package,
    /// they must respond with RemoteResponse::DownloadApproved,
    /// at which point requester can expect an FTWorkerRequest::Receive.
    Download {
        package_id: PackageId,
        desired_version_hash: Option<String>,
    },
}

/// The response expected from sending a [`RemoteRequest`].
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteResponse {
    DownloadApproved,
    DownloadDenied, // TODO expand on why
    Metadata,
}

/// Local requests sent to the app store take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalRequest {
    /// Expects a zipped package as blob, and creates a new package from it.
    ///
    /// If requested, will return a NewPackageResponse indicating success/failure.
    /// This is used for locally installing a package.
    /// TODO could switch this to Erc721Metadata
    NewPackage {
        package: PackageId,
        /// Sets whether we will mirror this package for others
        mirror: bool,
    },
    /// Try to download a package from a specified node.
    ///
    /// If requested, will return a DownloadResponse indicating success/failure.
    /// No blob is expected.
    Download {
        package: PackageId,
        download_from: NodeId,
        /// Sets whether we will mirror this package for others
        mirror: bool,
        /// Sets whether we will try to automatically update this package
        /// when a new version is posted to the listings contract
        auto_update: bool,
        /// The version hash we're looking for. If None, will download the latest.
        /// TODO could switch this to more friendly version numbers e.g 1.0.1 given new metadata structure
        desired_version_hash: Option<String>,
    },
    /// Select a downloaded package and install it. Will only succeed if the
    /// package is currently in the filesystem. If the package has *already*
    /// been installed, this will kill the running package and reset it with
    /// what's on disk.
    ///
    /// If requested, will return an InstallResponse indicating success/failure.
    /// No blob is expected.
    Install(PackageId),
    /// Select an installed package and uninstall it.
    /// This will kill the processes in the **manifest** of the package,
    /// but not the processes that were spawned by those processes! Take
    /// care to kill those processes yourself. This will also delete the drive
    /// containing the source code for this package. This does not guarantee
    /// that other data created by this package will be removed from places such
    /// as the key-value store.
    ///
    /// If requested, will return an UninstallResponse indicating success/failure.
    /// No blob is expected.
    Uninstall(PackageId),
    /// Start mirroring a package. This will fail if the package has not been downloaded.
    StartMirroring(PackageId),
    /// Stop mirroring a package. This will fail if the package has not been downloaded.
    StopMirroring(PackageId),
    /// Turn on automatic updates to a package. This will fail if the package has not been downloaded.
    StartAutoUpdate(PackageId),
    /// Turn off automatic updates to a package. This will fail if the package has not been downloaded.
    StopAutoUpdate(PackageId),
    /// This is an expensive operation! Throw away our state and rebuild from scratch.
    /// Re-index the locally downloaded/installed packages AND the onchain data.
    RebuildIndex,
}

/// Local responses take this form.
/// The variant of `LocalResponse` given will match the `LocalRequest` it is
/// responding to.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalResponse {
    NewPackageResponse(NewPackageResponse),
    DownloadResponse(DownloadResponse),
    InstallResponse(InstallResponse),
    UninstallResponse(UninstallResponse),
    MirrorResponse(MirrorResponse),
    AutoUpdateResponse(AutoUpdateResponse),
    RebuiltIndex,
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

#[derive(Debug, Serialize, Deserialize)]
pub enum MirrorResponse {
    Success,
    Failure,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AutoUpdateResponse {
    Success,
    Failure,
}
