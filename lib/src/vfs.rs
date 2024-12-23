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
    #[error("no write capability for requested drive")]
    NoWriteCap,
    #[error("no read capability for requested drive")]
    NoReadCap,
    #[error("failed to generate capability for new drive")]
    AddCapFailed,
    #[error("request could not be deserialized to valid VfsRequest")]
    MalformedRequest,
    #[error("request type used requires a blob")]
    NoBlob,
    #[error("error parsing path: {path}: {error}")]
    ParseError { error: String, path: String },
    #[error("IO error: {0}")]
    IOError(String),
    #[error("non-file non-dir in zip")]
    UnzipError,
}

impl From<std::io::Error> for VfsError {
    fn from(err: std::io::Error) -> Self {
        VfsError::IOError(err.to_string())
    }
}
