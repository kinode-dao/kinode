use super::bindings::component::uq_process::types as wit;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

//
// process-facing kernel types, used for process
// management and message-passing
// matches types in uqbar.wit
//

pub type Context = String; // JSON-string

#[derive(Clone, Debug, Eq, Hash, Serialize, Deserialize)]
pub enum ProcessId {
    Id(u64),
    Name(String),
}

impl PartialEq for ProcessId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ProcessId::Id(i1), ProcessId::Id(i2)) => i1 == i2,
            (ProcessId::Name(s1), ProcessId::Name(s2)) => s1 == s2,
            _ => false,
        }
    }
}
impl PartialEq<&str> for ProcessId {
    fn eq(&self, other: &&str) -> bool {
        match self {
            ProcessId::Id(_) => false,
            ProcessId::Name(s) => s == other,
        }
    }
}
impl PartialEq<u64> for ProcessId {
    fn eq(&self, other: &u64) -> bool {
        match self {
            ProcessId::Id(i) => i == other,
            ProcessId::Name(s) => false,
        }
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct Address {
    pub node: String,
    pub process: ProcessId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Payload {
    pub mime: Option<String>, // MIME type
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub inherit: bool,
    pub expects_response: Option<u64>,
    pub ipc: Option<String>,      // JSON-string
    pub metadata: Option<String>, // JSON-string
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub ipc: Option<String>,      // JSON-string
    pub metadata: Option<String>, // JSON-string
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Message {
    Request(Request),
    Response((Response, Option<Context>)),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct Capability {
    pub issuer: Address,
    pub params: String, // JSON-string
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct SignedCapability {
    pub issuer: Address,
    pub params: String,     // JSON-string
    pub signature: Vec<u8>, // signed by the kernel, so we can verify that the kernel issued it
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendError {
    pub kind: SendErrorKind,
    pub target: Address,
    pub message: Message,
    pub payload: Option<Payload>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SendErrorKind {
    Offline,
    Timeout,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OnPanic {
    None,
    Restart,
    Requests(Vec<(Address, Request)>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KernelCommand {
    StartProcess {
        name: Option<String>,
        wasm_bytes_handle: u128,
        on_panic: OnPanic,
        initial_capabilities: HashSet<SignedCapability>,
    },
    KillProcess(ProcessId), // this is extrajudicial killing: we might lose messages!
    RebootProcess {
        // kernel only
        process_id: ProcessId,
        persisted: PersistedProcess,
    },
    Shutdown,
    // capabilities creation
    GrantCapability {
        to_process: ProcessId,
        params: String, // JSON-string
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedProcess {
    pub wasm_bytes_handle: u128,
    pub on_panic: OnPanic,
    pub capabilities: HashSet<Capability>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsRequest {
    New {
        drive: String,
    },
    Add {
        drive: String,
        full_path: String,
        entry_type: AddEntryType,
    },
    Rename {
        drive: String,
        full_path: String,
        new_full_path: String,
    },
    Delete {
        drive: String,
        full_path: String,
    },
    WriteOffset {
        drive: String,
        full_path: String,
        offset: u64,
    },
    SetSize {
        drive: String,
        full_path: String,
        size: u64,
    },
    GetPath {
        drive: String,
        hash: u128,
    },
    GetHash {
        drive: String,
        full_path: String,
    },
    GetEntry {
        drive: String,
        full_path: String,
    },
    GetFileChunk {
        drive: String,
        full_path: String,
        offset: u64,
        length: u64,
    },
    GetEntryLength {
        drive: String,
        full_path: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AddEntryType {
    Dir,
    NewFile,                     //  add a new file to fs and add name in vfs
    ExistingFile { hash: u128 }, //  link an existing file in fs to a new name in vfs
    ZipArchive,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum GetEntryType {
    Dir,
    File,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsResponse {
    New {
        drive: String,
    },
    Add {
        drive: String,
        full_path: String,
    },
    Rename {
        drive: String,
        new_full_path: String,
    },
    Delete {
        drive: String,
        full_path: String,
    },
    WriteOffset {
        drive: String,
        full_path: String,
        offset: u64,
    },
    SetSize {
        drive: String,
        full_path: String,
        size: u64,
    },
    GetPath {
        drive: String,
        hash: u128,
        full_path: Option<String>,
    },
    GetHash {
        drive: String,
        full_path: String,
        hash: u128,
    },
    GetEntry {
        drive: String,
        full_path: String,
        children: Vec<String>,
    },
    GetFileChunk {
        drive: String,
        full_path: String,
        offset: u64,
        length: u64,
    },
    GetEntryLength {
        drive: String,
        full_path: String,
        length: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KeyValueMessage {
    New { drive: String },
    Write { drive: String, key: Vec<u8> },
    Read { drive: String, key: Vec<u8> },
}
impl KeyValueError {
    pub fn kind(&self) -> &str {
        match *self {
            KeyValueError::BadDriveName => "BadDriveName",
            KeyValueError::NoCap => "NoCap",
            KeyValueError::NoBytes => "NoBytes",
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub enum KeyValueError {
    BadDriveName,
    NoCap,
    NoBytes,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SqliteMessage {
    New { identifier: String },
    Write { identifier: String, key: Vec<u8> },
    Read { identifier: String, key: Vec<u8> },
}

//
// conversions between wit types and kernel types (annoying)
//

pub fn en_wit_process_id(process_id: ProcessId) -> wit::ProcessId {
    match process_id {
        ProcessId::Id(id) => wit::ProcessId::Id(id),
        ProcessId::Name(name) => wit::ProcessId::Name(name),
    }
}

pub fn de_wit_process_id(wit: wit::ProcessId) -> ProcessId {
    match wit {
        wit::ProcessId::Id(id) => ProcessId::Id(id),
        wit::ProcessId::Name(name) => ProcessId::Name(name),
    }
}

pub fn en_wit_address(address: Address) -> wit::Address {
    wit::Address {
        node: address.node,
        process: match address.process {
            ProcessId::Id(id) => wit::ProcessId::Id(id),
            ProcessId::Name(name) => wit::ProcessId::Name(name),
        },
    }
}

pub fn de_wit_address(wit: wit::Address) -> Address {
    Address {
        node: wit.node,
        process: match wit.process {
            wit::ProcessId::Id(id) => ProcessId::Id(id),
            wit::ProcessId::Name(name) => ProcessId::Name(name),
        },
    }
}

pub fn de_wit_request(wit: wit::Request) -> Request {
    Request {
        inherit: wit.inherit,
        expects_response: wit.expects_response,
        ipc: wit.ipc,
        metadata: wit.metadata,
    }
}

pub fn en_wit_request(request: Request) -> wit::Request {
    wit::Request {
        inherit: request.inherit,
        expects_response: request.expects_response,
        ipc: request.ipc,
        metadata: request.metadata,
    }
}

pub fn de_wit_response(wit: wit::Response) -> Response {
    Response {
        ipc: wit.ipc,
        metadata: wit.metadata,
    }
}

pub fn en_wit_response(response: Response) -> wit::Response {
    wit::Response {
        ipc: response.ipc,
        metadata: response.metadata,
    }
}

pub fn de_wit_payload(wit: Option<wit::Payload>) -> Option<Payload> {
    match wit {
        None => None,
        Some(wit) => Some(Payload {
            mime: wit.mime,
            bytes: wit.bytes,
        }),
    }
}

pub fn en_wit_payload(load: Option<Payload>) -> Option<wit::Payload> {
    match load {
        None => None,
        Some(load) => Some(wit::Payload {
            mime: load.mime,
            bytes: load.bytes,
        }),
    }
}

pub fn de_wit_signed_capability(wit: wit::SignedCapability) -> SignedCapability {
    SignedCapability {
        issuer: de_wit_address(wit.issuer),
        params: wit.params,
        signature: wit.signature,
    }
}

pub fn en_wit_signed_capability(cap: SignedCapability) -> wit::SignedCapability {
    wit::SignedCapability {
        issuer: en_wit_address(cap.issuer),
        params: cap.params,
        signature: cap.signature,
    }
}
