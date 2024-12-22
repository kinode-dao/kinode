use crate::types::core::CapMessage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// IPC Request format for the vfs:distro:sys runtime module.
#[derive(Debug, Serialize, Deserialize)]
pub struct VfsRequest {
    pub path: String,
    pub action: VfsAction,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum VfsAction {
    CreateDrive,
    CreateDir,
    CreateDirAll,
    CreateFile,
    OpenFile { create: bool },
    CloseFile,
    Write,
    WriteAll,
    Append,
    SyncAll,
    Read,
    ReadDir,
    ReadToEnd,
    ReadExact { length: u64 },
    ReadToString,
    Seek(SeekFrom),
    RemoveFile,
    RemoveDir,
    RemoveDirAll,
    Rename { new_path: String },
    Metadata,
    AddZip,
    CopyFile { new_path: String },
    Len,
    SetLen(u64),
    Hash,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_type: FileType,
    pub len: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DirEntry {
    pub path: String,
    pub file_type: FileType,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsResponse {
    Ok,
    Err(VfsError),
    Read,
    SeekFrom { new_offset: u64 },
    ReadDir(Vec<DirEntry>),
    ReadToString(String),
    Metadata(FileMetadata),
    Len(u64),
    Hash([u8; 32]),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum VfsError {
    #[error("No capability for action {action} at path {path}")]
    NoCap { action: String, path: String },
    #[error("Bytes blob required for {action} at path {path}")]
    BadBytes { action: String, path: String },
    #[error("bad request error: {error}")]
    BadRequest { error: String },
    #[error("error parsing path: {path}: {error}")]
    ParseError { error: String, path: String },
    #[error("IO error: {error}, at path {path}")]
    IOError { error: String, path: String },
    #[error("kernel capability channel error: {error}")]
    CapChannelFail { error: String },
    #[error("Bad JSON blob: {error}")]
    BadJson { error: String },
    #[error("File not found at path {path}")]
    NotFound { path: String },
    #[error("Creating directory failed at path: {path}: {error}")]
    CreateDirError { path: String, error: String },
    #[error("Other error: {error}")]
    Other { error: String },
}

impl VfsError {
    pub fn kind(&self) -> &str {
        match *self {
            VfsError::NoCap { .. } => "NoCap",
            VfsError::BadBytes { .. } => "BadBytes",
            VfsError::BadRequest { .. } => "BadRequest",
            VfsError::ParseError { .. } => "ParseError",
            VfsError::IOError { .. } => "IOError",
            VfsError::CapChannelFail { .. } => "CapChannelFail",
            VfsError::BadJson { .. } => "NoJson",
            VfsError::NotFound { .. } => "NotFound",
            VfsError::CreateDirError { .. } => "CreateDirError",
            VfsError::Other { .. } => "Other",
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for VfsError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        VfsError::CapChannelFail {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for VfsError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        VfsError::CapChannelFail {
            error: err.to_string(),
        }
    }
}

impl From<std::io::Error> for VfsError {
    fn from(err: std::io::Error) -> Self {
        VfsError::IOError {
            path: "".into(),
            error: err.to_string(),
        }
    }
}

impl std::fmt::Display for VfsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
