use crate::ft_worker_lib::{FTWorkerCommand, FTWorkerResult};
use serde::{Deserialize, Serialize};
use uqbar_process_lib::{NodeId, PackageId};

//
// app store API
//

/// The only Request type that this process will handle. Note that the
/// top-level label is not represented in JSON. These should be serialized
/// as JSON bytes. FTWorker requests will only be accepted by subprocesses
/// this process spawns, never send them. See the [`LocalRequest`] and
/// [`RemoteRequest`] types for what kind of Responses to expect.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    FTWorkerCommand(FTWorkerCommand),
    FTWorkerResult(FTWorkerResult),
}

/// The only Response type this process will issue. Note that the top-level
/// label is not represented in JSON. These will be serialized as JSON bytes.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all responses
pub enum Resp {
    RemoteResponse(RemoteResponse),
    FTWorkerResult(FTWorkerResult),
    // note that we do not need to ourselves handle local responses, as
    // those are given to others rather than received.
    NewPackageResponse(NewPackageResponse),
    DownloadResponse(DownloadResponse),
    InstallResponse(InstallResponse),
}

/// Local Requests take this form. `NewPackage`, `Download`, and `Install` will
/// return Responses, while `Uninstall` and `Delete` will not.
#[derive(Debug, Serialize, Deserialize)]
pub enum LocalRequest {
    /// expects a zipped package as payload: create a new package from it
    /// if requested, will return a NewPackageResponse indicating success/failure
    NewPackage {
        package: PackageId,
        mirror: bool, // sets whether we will mirror this package
    },
    /// no payload; try to download a package from a specified node
    /// if requested, will return a DownloadResponse indicating success/failure
    Download {
        package: PackageId,
        install_from: NodeId,
    },
    /// no payload; select a downloaded package and install it
    /// if requested, will return an InstallResponse indicating success/failure
    Install(PackageId),
    /// no payload; select an installed package and uninstall it
    /// no response will be given
    Uninstall(PackageId),
    /// no payload; select a downloaded package and delete it
    /// no response will be given
    Delete(PackageId),
}

/// Remote requests, those sent between instantiations of this process
/// on different nodes, take this form.
#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteRequest {
    /// no payload; request a package from a node
    /// remote node must return RemoteResponse::DownloadApproved,
    /// at which point requester can expect a FTWorkerRequest::Receive
    Download(PackageId),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoteResponse {
    DownloadApproved,
    DownloadDenied, // TODO expand on why
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
