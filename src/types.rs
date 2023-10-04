use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use thiserror::Error;
use tokio::sync::RwLock;

//
// internal message pipes between kernel and runtime modules
//

// keeps the from address so we know where to pipe error
pub type NetworkErrorSender = tokio::sync::mpsc::Sender<WrappedSendError>;
pub type NetworkErrorReceiver = tokio::sync::mpsc::Receiver<WrappedSendError>;

pub type MessageSender = tokio::sync::mpsc::Sender<KernelMessage>;
pub type MessageReceiver = tokio::sync::mpsc::Receiver<KernelMessage>;

pub type PrintSender = tokio::sync::mpsc::Sender<Printout>;
pub type PrintReceiver = tokio::sync::mpsc::Receiver<Printout>;

pub type DebugSender = tokio::sync::mpsc::Sender<DebugCommand>;
pub type DebugReceiver = tokio::sync::mpsc::Receiver<DebugCommand>;

pub type CapMessageSender = tokio::sync::mpsc::UnboundedSender<CapMessage>;
pub type CapMessageReceiver = tokio::sync::mpsc::UnboundedReceiver<CapMessage>;

//
// types used for UQI: uqbar's identity system
//
pub type PKINames = Arc<RwLock<HashMap<String, String>>>; // TODO maybe U256 to String
pub type OnchainPKI = Arc<RwLock<HashMap<String, Identity>>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Registration {
    pub username: String,
    pub password: String,
    pub direct: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub name: String,
    pub networking_key: String,
    pub ws_routing: Option<(String, u16)>,
    pub allowed_routers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityTransaction {
    pub from: String,
    pub signature: Option<String>,
    pub to: String, // contract address
    pub town_id: u32,
    pub calldata: Identity,
    pub nonce: String,
}

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
            ProcessId::Name(_) => false,
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
    pub expects_response: Option<u64>, // number of seconds until timeout
    pub ipc: Option<String>,           // JSON-string
    pub metadata: Option<String>,      // JSON-string
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
    pub target: Address, // what the message was trying to reach
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
    Requests(Vec<(Address, Request, Option<Payload>)>),
}

impl OnPanic {
    pub fn is_restart(&self) -> bool {
        match self {
            OnPanic::None => false,
            OnPanic::Restart => true,
            OnPanic::Requests(_) => false,
        }
    }
}

//
// kernel types that runtime modules use
//

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessMetadata {
    pub our: Address,
    pub wasm_bytes_handle: u128,
    pub on_panic: OnPanic,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelMessage {
    pub id: u64,
    pub source: Address,
    pub target: Address,
    pub rsvp: Rsvp,
    pub message: Message,
    pub payload: Option<Payload>,
    pub signed_capabilities: Option<Vec<SignedCapability>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WrappedSendError {
    pub id: u64,
    pub source: Address,
    pub error: SendError,
}

/// A terminal printout. Verbosity level is from low to high, and for
/// now, only 0 and 1 are used. Level 0 is always printed, level 1 is
/// only printed if the terminal is in verbose mode. Numbers greater
/// than 1 are reserved for future use and will be ignored for now.
pub struct Printout {
    pub verbosity: u8,
    pub content: String,
}

//  kernel sets in case, e.g.,
//   A requests response from B does not request response from C
//   -> kernel sets `Some(A) = Rsvp` for B's request to C
pub type Rsvp = Option<Address>;

//
//  boot/startup specific types???
//

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SequentializeRequest {
    QueueMessage(QueueMessage),
    RunQueue,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueueMessage {
    pub target: ProcessId,
    pub request: Request,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BootOutboundRequest {
    pub target_process: ProcessId,
    pub json: Option<String>,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DebugCommand {
    Toggle,
    Step,
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
    // kernel only
    RebootProcess {
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

pub enum CapMessage {
    Add {
        on: ProcessId,
        cap: Capability,
    },
    Drop {
        // not used yet!
        on: ProcessId,
        cap: Capability,
    },
    Has {
        // a bool is given in response here
        on: ProcessId,
        cap: Capability,
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    GetAll {
        on: ProcessId,
        responder: tokio::sync::oneshot::Sender<HashSet<Capability>>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KernelResponse {
    StartedProcess(ProcessMetadata),
    KilledProcess(ProcessId),
}

pub type ProcessMap = HashMap<ProcessId, PersistedProcess>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedProcess {
    pub wasm_bytes_handle: u128,
    // pub identifier: String,
    // pub full_path: String,
    pub on_panic: OnPanic,
    pub capabilities: HashSet<Capability>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessContext {
    // store ultimate in order to set prompting message if needed
    pub prompting_message: Option<KernelMessage>,
    // can be empty if a request doesn't set context, but still needs to inherit
    pub context: Option<Context>,
}

//
// runtime-module-specific types
//

//
// filesystem.rs types
//

#[derive(Serialize, Deserialize, Debug)]
pub enum FsAction {
    Write,
    Replace(u128),
    WriteOffset((u128, u64)),
    Append(Option<u128>),
    Read(u128),
    ReadChunk(ReadChunkRequest),
    Delete(u128),
    Length(u128),
    SetLength((u128, u64)),
    GetState,
    SetState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReadChunkRequest {
    pub file: u128,
    pub start: u64,
    pub length: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum FsResponse {
    Write(u128),
    Read(u128),
    ReadChunk(u128),
    Append(u128),
    Delete(u128),
    Length(u64),
    GetState,
    SetState,
}

#[derive(Debug)]
pub struct S3Config {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub bucket: String,
    pub endpoint: String,
}

#[derive(Debug)]
pub struct FsConfig {
    pub s3_config: Option<S3Config>,
    pub mem_buffer_limit: usize,
    pub chunk_size: usize,
    pub flush_to_cold_interval: usize,
    pub encryption: bool,
    pub cloud_enabled: bool,
    // pub flush_to_wal_interval: usize,
}

impl VfsError {
    pub fn kind(&self) -> &str {
        match *self {
            VfsError::BadIdentifier => "BadIdentifier",
            VfsError::BadDescriptor => "BadDescriptor",
            VfsError::NoCap => "NoCap",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsError {
    BadIdentifier,
    BadDescriptor,
    NoCap,
}

impl FileSystemError {
    pub fn kind(&self) -> &str {
        match *self {
            FileSystemError::BadUri { .. } => "BadUri",
            FileSystemError::BadJson { .. } => "BadJson",
            FileSystemError::BadBytes { .. } => "BadBytes",
            FileSystemError::IllegalAccess { .. } => "IllegalAccess",
            FileSystemError::AlreadyOpen { .. } => "AlreadyOpen",
            FileSystemError::NotCurrentlyOpen { .. } => "NotCurrentlyOpen",
            FileSystemError::BadPathJoin { .. } => "BadPathJoin",
            FileSystemError::CouldNotMakeDir { .. } => "CouldNotMakeDir",
            FileSystemError::ReadFailed { .. } => "ReadFailed",
            FileSystemError::WriteFailed { .. } => "WriteFailed",
            FileSystemError::OpenFailed { .. } => "OpenFailed",
            FileSystemError::FsError { .. } => "FsError",
            FileSystemError::LFSError { .. } => "LFSErrror",
        }
    }
}

#[derive(Clone, Error, Debug, Serialize, Deserialize)]
pub enum FileSystemError {
    //  bad input from user
    #[error("Malformed URI: {uri}. Problem with {bad_part_name}: {:?}.", bad_part)]
    BadUri {
        uri: String,
        bad_part_name: String,
        bad_part: Option<String>,
    },
    #[error(
        "JSON payload could not be parsed to FileSystemRequest: {error}. Got {:?}.",
        json
    )]
    BadJson { json: String, error: String },
    #[error("Bytes payload required for {action}.")]
    BadBytes { action: String },
    #[error("{process_name} not allowed to access {attempted_dir}. Process may only access within {sandbox_dir}.")]
    IllegalAccess {
        process_name: String,
        attempted_dir: String,
        sandbox_dir: String,
    },
    #[error("Already have {path} opened with mode {:?}.", mode)]
    AlreadyOpen { path: String, mode: FileSystemMode },
    #[error("Don't have {path} opened with mode {:?}.", mode)]
    NotCurrentlyOpen { path: String, mode: FileSystemMode },
    //  path or underlying fs problems
    #[error("Failed to join path: base: '{base_path}'; addend: '{addend}'.")]
    BadPathJoin { base_path: String, addend: String },
    #[error("Failed to create dir at {path}: {error}.")]
    CouldNotMakeDir { path: String, error: String },
    #[error("Failed to read {path}: {error}.")]
    ReadFailed { path: String, error: String },
    #[error("Failed to write {path}: {error}.")]
    WriteFailed { path: String, error: String },
    #[error("Failed to open {path} for {:?}: {error}.", mode)]
    OpenFailed {
        path: String,
        mode: FileSystemMode,
        error: String,
    },
    #[error("Filesystem error while {what} on {path}: {error}.")]
    FsError {
        what: String,
        path: String,
        error: String,
    },
    #[error("LFS error: {error}.")]
    LFSError { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileSystemRequest {
    pub uri_string: String,
    pub action: FileSystemAction,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FileSystemAction {
    Read,
    Write,
    GetMetadata,
    ReadDir,
    Open(FileSystemMode),
    Close(FileSystemMode),
    Append,
    ReadChunkFromOpen(u64),
    SeekWithinOpen(FileSystemSeekFrom),
}

//  copy of std::io::SeekFrom with Serialize/Deserialize
#[derive(Debug, Serialize, Deserialize)]
pub enum FileSystemSeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FileSystemResponse {
    Read(FileSystemUriHash),
    Write(String),
    GetMetadata(FileSystemMetadata),
    ReadDir(Vec<FileSystemMetadata>),
    Open {
        uri_string: String,
        mode: FileSystemMode,
    },
    Close {
        uri_string: String,
        mode: FileSystemMode,
    },
    Append(String),
    ReadChunkFromOpen(FileSystemUriHash),
    SeekWithinOpen(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileSystemUriHash {
    pub uri_string: String,
    pub hash: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileSystemMetadata {
    pub uri_string: String,
    pub hash: Option<u64>,
    pub entry_type: FileSystemEntryType,
    pub len: u64,
}

#[derive(Eq, Hash, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub enum FileSystemMode {
    Read,
    Append,
    AppendOverwrite,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FileSystemEntryType {
    Symlink,
    File,
    Dir,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsRequest {
    New {
        identifier: String,
    },
    Add {
        identifier: String,
        full_path: String,
        entry_type: AddEntryType,
    },
    Rename {
        identifier: String,
        full_path: String,
        new_full_path: String,
    },
    Delete {
        identifier: String,
        full_path: String,
    },
    WriteOffset {
        identifier: String,
        full_path: String,
        offset: u64,
    },
    SetSize {
        identifier: String,
        full_path: String,
        size: u64,
    },
    GetPath {
        identifier: String,
        hash: u128,
    },
    GetHash {
        identifier: String,
        full_path: String,
    },
    GetEntry {
        identifier: String,
        full_path: String,
    },
    GetFileChunk {
        identifier: String,
        full_path: String,
        offset: u64,
        length: u64,
    },
    GetEntryLength {
        identifier: String,
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
        identifier: String,
    },
    Add {
        identifier: String,
        full_path: String,
    },
    Rename {
        identifier: String,
        new_full_path: String,
    },
    Delete {
        identifier: String,
        full_path: String,
    },
    WriteOffset {
        identifier: String,
        full_path: String,
        offset: u64,
    },
    SetSize {
        identifier: String,
        full_path: String,
        size: u64,
    },
    GetPath {
        identifier: String,
        hash: u128,
        full_path: Option<String>,
    },
    GetHash {
        identifier: String,
        full_path: String,
        hash: u128,
    },
    GetEntry {
        identifier: String,
        full_path: String,
        children: Vec<String>,
    },
    GetFileChunk {
        identifier: String,
        full_path: String,
        offset: u64,
        length: u64,
    },
    GetEntryLength {
        identifier: String,
        full_path: String,
        length: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KeyValueMessage {
    New { identifier: String },
    Write { identifier: String, key: Vec<u8> },
    Read { identifier: String, key: Vec<u8> },
}
impl KeyValueError {
    pub fn kind(&self) -> &str {
        match *self {
            KeyValueError::BadIdentifier => "BadIdentifier",
            KeyValueError::NoCap => "NoCap",
            KeyValueError::NoBytes => "NoBytes",
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub enum KeyValueError {
    BadIdentifier,
    NoCap,
    NoBytes,
}

//
// http_client.rs types
//

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpClientRequest {
    pub uri: String,
    pub method: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpClientResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpClientError {
    #[error("http_client: rsvp is None but message is expecting response")]
    BadRsvp,
    #[error("http_client: no json in request")]
    NoJson,
    #[error(
        "http_client: JSON payload could not be parsed to HttpClientRequest: {error}. Got {:?}.",
        json
    )]
    BadJson { json: String, error: String },
    #[error("http_client: http method not supported: {:?}", method)]
    BadMethod { method: String },
    #[error("http_client: failed to execute request {:?}", error)]
    RequestFailed { error: String },
}

impl HttpClientError {
    pub fn kind(&self) -> &str {
        match *self {
            HttpClientError::BadRsvp { .. } => "BadRsvp",
            HttpClientError::NoJson { .. } => "NoJson",
            HttpClientError::BadJson { .. } => "BadJson",
            HttpClientError::BadMethod { .. } => "BadMethod",
            HttpClientError::RequestFailed { .. } => "RequestFailed",
        }
    }
}

//
// custom kernel displays
//

impl std::fmt::Display for KernelMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{{\n    id: {},\n    source: {},\n    target: {},\n    rsvp: {:?},\n    message: {}\n}}",
            self.id, self.source, self.target, self.rsvp, self.message,
        )
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Message::Request(request) => write!(
                f,
                "Request(\n        inherit: {},\n        expects_response: {:?},\n        ipc: {},\n        metadata: {}\n    )",
                request.inherit,
                request.expects_response,
                &request.ipc.as_ref().unwrap_or(&"None".into()),
                &request.metadata.as_ref().unwrap_or(&"None".into()),
            ),
            Message::Response((response, context)) => write!(
                f,
                "Response(\n        ipc: {},\n        metadata: {},\n        context: {}\n    )",
                &response.ipc.as_ref().unwrap_or(&"None".into()),
                &response.metadata.as_ref().unwrap_or(&"None".into()),
                &context.as_ref().unwrap_or(&"None".into()),
            ),
        }
    }
}

//
// http_server.rs types
//

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>, // TODO does this use a lot of memory?
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpServerError {
    #[error("http_server: json is None")]
    NoJson,
    #[error("http_server: response not ok")]
    ResponseError,
    #[error("http_server: bytes are None")]
    NoBytes,
    #[error(
        "http_server: JSON payload could not be parsed to HttpClientRequest: {error}. Got {:?}.",
        json
    )]
    BadJson { json: String, error: String },
}

impl HttpServerError {
    pub fn kind(&self) -> &str {
        match *self {
            HttpServerError::NoJson { .. } => "NoJson",
            HttpServerError::NoBytes { .. } => "NoBytes",
            HttpServerError::BadJson { .. } => "BadJson",
            HttpServerError::ResponseError { .. } => "ResponseError",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub username: String,
    pub expiration: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebSocketServerTarget {
    pub node: String,
    pub id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WebSocketPush {
    pub target: WebSocketServerTarget,
    pub is_text: Option<bool>,
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Address {
                    node,
                    process: ProcessId::Id(id),
                } => format!("{}/{}", node, id),
                Address {
                    node,
                    process: ProcessId::Name(name),
                } => format!("{}/{}", node, name),
            }
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerAction {
    pub action: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum HttpServerMessage {
    WebSocketPush(WebSocketPush),
    ServerAction(ServerAction),
    WsRegister(WsRegister),                 // Coming from a proxy
    WsProxyDisconnect(WsProxyDisconnect),   // Coming from a proxy
    WsMessage(WsMessage),                   // Coming from a proxy
    EncryptedWsMessage(EncryptedWsMessage), // Coming from a proxy
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsRegister {
    pub ws_auth_token: String,
    pub auth_token: String,
    pub channel_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsProxyDisconnect {
    // Doesn't require auth because it's coming from the proxy
    pub channel_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsMessage {
    pub ws_auth_token: String,
    pub auth_token: String,
    pub channel_id: String,
    pub target: Address,
    pub json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedWsMessage {
    pub ws_auth_token: String,
    pub auth_token: String,
    pub channel_id: String,
    pub target: Address,
    pub encrypted: String, // Encrypted JSON as hex with the 32-byte authentication tag appended
    pub nonce: String,     // Hex of the 12-byte nonce
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WebSocketClientMessage {
    WsRegister(WsRegister),
    WsMessage(WsMessage),
    EncryptedWsMessage(EncryptedWsMessage),
}
// http_server End

// encryptor Start
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetKeyAction {
    pub channel_id: String,
    pub public_key_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecryptAndForwardAction {
    pub channel_id: String,
    pub forward_to: Address, // node, process
    pub json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptAndForwardAction {
    pub channel_id: String,
    pub forward_to: Address, // node, process
    pub json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecryptAction {
    pub channel_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptAction {
    pub channel_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EncryptorMessage {
    GetKeyAction(GetKeyAction),
    DecryptAndForwardAction(DecryptAndForwardAction),
    EncryptAndForwardAction(EncryptAndForwardAction),
    DecryptAction(DecryptAction),
    EncryptAction(EncryptAction),
}
// encryptor End
