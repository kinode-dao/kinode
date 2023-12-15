use crate::kernel::process::wit;
use ring::signature;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

lazy_static::lazy_static! {
    pub static ref ENCRYPTOR_PROCESS_ID: ProcessId = ProcessId::new(Some("encryptor"), "sys", "uqbar");
    pub static ref ETH_RPC_PROCESS_ID: ProcessId = ProcessId::new(Some("eth_rpc"), "sys", "uqbar");
    pub static ref FILESYSTEM_PROCESS_ID: ProcessId = ProcessId::new(Some("filesystem"), "sys", "uqbar");
    pub static ref HTTP_CLIENT_PROCESS_ID: ProcessId = ProcessId::new(Some("http_client"), "sys", "uqbar");
    pub static ref HTTP_SERVER_PROCESS_ID: ProcessId = ProcessId::new(Some("http_server"), "sys", "uqbar");
    pub static ref KERNEL_PROCESS_ID: ProcessId = ProcessId::new(Some("kernel"), "sys", "uqbar");
    pub static ref TERMINAL_PROCESS_ID: ProcessId = ProcessId::new(Some("terminal"), "terminal", "uqbar");
    pub static ref TIMER_PROCESS_ID: ProcessId = ProcessId::new(Some("timer"), "sys", "uqbar");
    pub static ref VFS_PROCESS_ID: ProcessId = ProcessId::new(Some("vfs"), "sys", "uqbar");
    pub static ref STATE_PROCESS_ID: ProcessId = ProcessId::new(Some("state"), "sys", "uqbar");

}

//
// types shared between kernel and processes. frustratingly, this is an exact copy
// of the types in process_lib
// this is because even though the types are identical, they will not match when
// used in the kernel context which generates bindings differently than the process
// standard library. make sure to keep this synced with process_lib.
//
pub type Context = Vec<u8>;
pub type NodeId = String; // QNS domain name

/// process ID is a formatted unique identifier that contains
/// the publishing node's ID, the package name, and finally the process name.
/// the process name can be a random number, or a name chosen by the user.
/// the formatting is as follows:
/// `[process name]:[package name]:[node ID]`
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProcessId {
    process_name: String,
    package_name: String,
    publisher_node: NodeId,
}

impl Serialize for ProcessId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        format!("{}", self).serialize(serializer)
    }
}

impl<'a> Deserialize<'a> for ProcessId {
    fn deserialize<D>(deserializer: D) -> Result<ProcessId, D::Error>
    where
        D: serde::de::Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;
        ProcessId::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// PackageId is like a ProcessId, but for a package. Only contains the name
/// of the package and the name of the publisher.
#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct PackageId {
    package_name: String,
    publisher_node: String,
}

impl PackageId {
    pub fn new(package_name: &str, publisher_node: &str) -> Self {
        PackageId {
            package_name: package_name.into(),
            publisher_node: publisher_node.into(),
        }
    }
    pub fn from_str(input: &str) -> Result<Self, ProcessIdParseError> {
        // split string on colons into 2 segments
        let mut segments = input.split(':');
        let package_name = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        let publisher_node = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        if segments.next().is_some() {
            return Err(ProcessIdParseError::TooManyColons);
        }
        Ok(PackageId {
            package_name,
            publisher_node,
        })
    }
    pub fn _package(&self) -> &str {
        &self.package_name
    }
    pub fn _publisher(&self) -> &str {
        &self.publisher_node
    }
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.package_name, self.publisher_node)
    }
}

/// ProcessId is defined in the wit bindings, but constructors and methods
/// are defined here.
impl ProcessId {
    /// generates a random u64 number if process_name is not declared
    pub fn new(process_name: Option<&str>, package_name: &str, publisher_node: &str) -> Self {
        ProcessId {
            process_name: process_name
                .unwrap_or(&rand::random::<u64>().to_string())
                .into(),
            package_name: package_name.into(),
            publisher_node: publisher_node.into(),
        }
    }
    pub fn from_str(input: &str) -> Result<Self, ProcessIdParseError> {
        // split string on colons into 3 segments
        let mut segments = input.split(':');
        let process_name = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        let package_name = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        let publisher_node = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        if segments.next().is_some() {
            return Err(ProcessIdParseError::TooManyColons);
        }
        Ok(ProcessId {
            process_name,
            package_name,
            publisher_node,
        })
    }
    pub fn process(&self) -> &str {
        &self.process_name
    }
    pub fn package(&self) -> &str {
        &self.package_name
    }
    pub fn publisher(&self) -> &str {
        &self.publisher_node
    }
    pub fn en_wit(&self) -> wit::ProcessId {
        wit::ProcessId {
            process_name: self.process_name.clone(),
            package_name: self.package_name.clone(),
            publisher_node: self.publisher_node.clone(),
        }
    }
    pub fn de_wit(wit: wit::ProcessId) -> ProcessId {
        ProcessId {
            process_name: wit.process_name,
            package_name: wit.package_name,
            publisher_node: wit.publisher_node,
        }
    }
}

impl From<(&str, &str, &str)> for ProcessId {
    fn from(input: (&str, &str, &str)) -> Self {
        ProcessId::new(Some(input.0), input.1, input.2)
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.process_name, self.package_name, self.publisher_node
        )
    }
}

// impl PartialEq for ProcessId {
//     fn eq(&self, other: &Self) -> bool {
//         self.process_name == other.process_name
//             && self.package_name == other.package_name
//             && self.publisher_node == other.publisher_node
//     }
// }

impl PartialEq<&str> for ProcessId {
    fn eq(&self, other: &&str) -> bool {
        &self.to_string() == other
    }
}

impl PartialEq<ProcessId> for &str {
    fn eq(&self, other: &ProcessId) -> bool {
        self == &other.to_string()
    }
}

#[derive(Debug)]
pub enum ProcessIdParseError {
    TooManyColons,
    MissingField,
}

impl std::fmt::Display for ProcessIdParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ProcessIdParseError::TooManyColons => "Too many colons in ProcessId string",
                ProcessIdParseError::MissingField => "Missing field in ProcessId string",
            }
        )
    }
}

impl std::error::Error for ProcessIdParseError {
    fn description(&self) -> &str {
        match self {
            ProcessIdParseError::TooManyColons => "Too many colons in ProcessId string",
            ProcessIdParseError::MissingField => "Missing field in ProcessId string",
        }
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct Address {
    pub node: NodeId,
    pub process: ProcessId,
}

impl Address {
    pub fn new<T>(node: &str, process: T) -> Address
    where
        T: Into<ProcessId>,
    {
        Address {
            node: node.to_string(),
            process: process.into(),
        }
    }
    pub fn from_str(input: &str) -> Result<Self, AddressParseError> {
        // split string on colons into 4 segments,
        // first one with @, next 3 with :
        let mut name_rest = input.split('@');
        let node = name_rest
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        let mut segments = name_rest
            .next()
            .ok_or(AddressParseError::MissingNodeId)?
            .split(':');
        let process_name = segments
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        let package_name = segments
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        let publisher_node = segments
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        if segments.next().is_some() {
            return Err(AddressParseError::TooManyColons);
        }
        Ok(Address {
            node,
            process: ProcessId {
                process_name,
                package_name,
                publisher_node,
            },
        })
    }
    pub fn en_wit(&self) -> wit::Address {
        wit::Address {
            node: self.node.clone(),
            process: self.process.en_wit(),
        }
    }
    pub fn de_wit(wit: wit::Address) -> Address {
        Address {
            node: wit.node,
            process: ProcessId {
                process_name: wit.process.process_name,
                package_name: wit.process.package_name,
                publisher_node: wit.process.publisher_node,
            },
        }
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        format!("{}", self).serialize(serializer)
    }
}

impl<'a> Deserialize<'a> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Address, D::Error>
    where
        D: serde::de::Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;
        Address::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl From<(&str, &str, &str, &str)> for Address {
    fn from(input: (&str, &str, &str, &str)) -> Self {
        Address::new(input.0, (input.1, input.2, input.3))
    }
}

impl<T> From<(&str, T)> for Address
where
    T: Into<ProcessId>,
{
    fn from(input: (&str, T)) -> Self {
        Address::new(input.0, input.1)
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.node, self.process)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum AddressParseError {
    TooManyColons,
    MissingNodeId,
    MissingField,
}

impl std::fmt::Display for AddressParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                AddressParseError::TooManyColons => "Too many colons in ProcessId string",
                AddressParseError::MissingNodeId => "Node ID missing",
                AddressParseError::MissingField => "Missing field in ProcessId string",
            }
        )
    }
}

impl std::error::Error for AddressParseError {
    fn description(&self) -> &str {
        match self {
            AddressParseError::TooManyColons => "Too many colons in ProcessId string",
            AddressParseError::MissingNodeId => "Node ID missing",
            AddressParseError::MissingField => "Missing field in ProcessId string",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Payload {
    pub mime: Option<String>, // MIME type
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub inherit: bool,
    pub expects_response: Option<u64>, // number of seconds until timeout
    pub ipc: Vec<u8>,
    pub metadata: Option<String>, // JSON-string
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub inherit: bool,
    pub ipc: Vec<u8>,
    pub metadata: Option<String>, // JSON-string
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Message::Request(request) => write!(
                f,
                "Request(\n        inherit: {},\n        expects_response: {:?},\n        ipc: {},\n        metadata: {}\n    )",
                request.inherit,
                request.expects_response,
                match serde_json::from_slice::<serde_json::Value>(&request.ipc) {
                    Ok(json) => format!("{}", json),
                    Err(_) => format!("{:?}", request.ipc),
                },
                &request.metadata.as_ref().unwrap_or(&"None".into()),
            ),
            Message::Response((response, context)) => write!(
                f,
                "Response(\n        inherit: {},\n        ipc: {},\n        metadata: {},\n        context: {}\n    )",
                response.inherit,
                match serde_json::from_slice::<serde_json::Value>(&response.ipc) {
                    Ok(json) => format!("{}", json),
                    Err(_) => format!("{:?}", response.ipc),
                },
                &response.metadata.as_ref().unwrap_or(&"None".into()),
                if context.is_none() {
                    "None".into()
                } else {
                    match serde_json::from_slice::<serde_json::Value>(context.as_ref().unwrap()) {
                        Ok(json) => format!("{}", json),
                        Err(_) => format!("{:?}", context.as_ref().unwrap()),
                    }
                },
            ),
        }
    }
}

//
// conversions between wit types and kernel types (annoying!)
//

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
        inherit: wit.inherit,
        ipc: wit.ipc,
        metadata: wit.metadata,
    }
}

pub fn en_wit_response(response: Response) -> wit::Response {
    wit::Response {
        inherit: response.inherit,
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
        issuer: Address {
            node: wit.issuer.node,
            process: ProcessId {
                process_name: wit.issuer.process.process_name,
                package_name: wit.issuer.process.package_name,
                publisher_node: wit.issuer.process.publisher_node,
            },
        },
        params: wit.params,
        signature: wit.signature,
    }
}

pub fn _en_wit_signed_capability(cap: SignedCapability) -> wit::SignedCapability {
    wit::SignedCapability {
        issuer: cap.issuer.en_wit(),
        params: cap.params,
        signature: cap.signature,
    }
}

pub fn en_wit_message(message: Message) -> wit::Message {
    match message {
        Message::Request(request) => wit::Message::Request(en_wit_request(request)),
        Message::Response((response, context)) => {
            wit::Message::Response((en_wit_response(response), context))
        }
    }
}

pub fn en_wit_send_error(error: SendError) -> wit::SendError {
    wit::SendError {
        kind: en_wit_send_error_kind(error.kind),
        message: en_wit_message(error.message),
        payload: en_wit_payload(error.payload),
    }
}

pub fn en_wit_send_error_kind(kind: SendErrorKind) -> wit::SendErrorKind {
    match kind {
        SendErrorKind::Offline => wit::SendErrorKind::Offline,
        SendErrorKind::Timeout => wit::SendErrorKind::Timeout,
    }
}

pub fn de_wit_on_panic(wit: wit::OnPanic) -> OnPanic {
    match wit {
        wit::OnPanic::None => OnPanic::None,
        wit::OnPanic::Restart => OnPanic::Restart,
        wit::OnPanic::Requests(reqs) => OnPanic::Requests(
            reqs.into_iter()
                .map(|(address, request, payload)| {
                    (
                        Address::de_wit(address),
                        de_wit_request(request),
                        de_wit_payload(payload),
                    )
                })
                .collect(),
        ),
    }
}
//
// END SYNC WITH process_lib
//

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

pub type CapMessageSender = tokio::sync::mpsc::Sender<CapMessage>;
pub type CapMessageReceiver = tokio::sync::mpsc::Receiver<CapMessage>;

//
// types used for UQI: uqbar's identity system
//

#[derive(Debug, Serialize, Deserialize)]
pub struct Registration {
    pub username: NodeId,
    pub password: String,
    pub direct: bool,
}

#[derive(Debug)]
pub struct Keyfile {
    pub username: String,
    pub routers: Vec<String>,
    pub networking_keypair: signature::Ed25519KeyPair,
    pub jwt_secret_bytes: Vec<u8>,
    pub file_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyfileVet {
    pub password: String,
    pub keyfile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyfileVetted {
    pub username: String,
    pub networking_key: String,
    pub routers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootInfo {
    pub password: String,
    pub username: String,
    pub reset: bool,
    pub direct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportKeyfileInfo {
    pub password: String,
    pub keyfile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginInfo {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginAndResetInfo {
    pub password: String,
    pub direct: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub name: NodeId,
    pub networking_key: String,
    pub ws_routing: Option<(String, u16)>,
    pub allowed_routers: Vec<NodeId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnencryptedIdentity {
    pub name: NodeId,
    pub allowed_routers: Vec<NodeId>,
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
// kernel types that runtime modules use
//

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessMetadata {
    pub our: Address,
    pub wasm_bytes_handle: String,
    pub on_panic: OnPanic,
    pub public: bool,
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

impl std::fmt::Display for KernelMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{{\n    id: {},\n    source: {},\n    target: {},\n    rsvp: {},\n    message: {},\n    payload: {}\n}}",
            self.id,
            self.source,
            self.target,
            match &self.rsvp {
                Some(rsvp) => rsvp.to_string(),
                None => "None".to_string()
            },
            self.message,
            self.payload.is_some(),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WrappedSendError {
    pub id: u64,
    pub source: Address,
    pub error: SendError,
}

/// A terminal printout. Verbosity level is from low to high.
/// - `0`: always printed
/// - `1`: verbose, used for debugging
/// - `2`: very verbose: shows runtime information
/// - `3`: very verbose: shows every event in event loop
pub struct Printout {
    pub verbosity: u8,
    pub content: String,
}

//  kernel sets in case, e.g.,
//   A requests response from B does not request response from C
//   -> kernel sets `Some(A) = Rsvp` for B's request to C
pub type Rsvp = Option<Address>;

#[derive(Debug, Serialize, Deserialize)]
pub enum DebugCommand {
    Toggle,
    Step,
}

/// IPC format for requests sent to kernel runtime module
#[derive(Debug, Serialize, Deserialize)]
pub enum KernelCommand {
    /// RUNTIME ONLY: used to notify the kernel that booting is complete and
    /// all processes have been loaded in from their persisted or bootstrapped state.
    Booted,
    /// Tell the kernel to install and prepare a new process for execution.
    /// The process will not begin execution until the kernel receives a
    /// `RunProcess` command with the same `id`.
    ///
    /// The process that sends this command will be given messaging capabilities
    /// for the new process if `public` is false.
    InitializeProcess {
        id: ProcessId,
        wasm_bytes_handle: String,
        on_panic: OnPanic,
        initial_capabilities: HashSet<SignedCapability>,
        public: bool,
    },
    /// Tell the kernel to run a process that has already been installed.
    /// TODO: in the future, this command could be extended to allow for
    /// resource provision.
    RunProcess(ProcessId),
    /// Kill a running process immediately. This may result in the dropping / mishandling of messages!
    KillProcess(ProcessId),
    /// RUNTIME ONLY: notify the kernel that the runtime is shutting down and it
    /// should gracefully stop and persist the running processes.
    Shutdown,
}

/// IPC format for all KernelCommand responses
#[derive(Debug, Serialize, Deserialize)]
pub enum KernelResponse {
    InitializedProcess,
    InitializeProcessError,
    StartedProcess,
    RunProcessError,
    KilledProcess(ProcessId),
}

#[derive(Debug)]
pub enum CapMessage {
    Add {
        on: ProcessId,
        cap: Capability,
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    _Drop {
        // not used yet!
        on: ProcessId,
        cap: Capability,
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    Has {
        // a bool is given in response here
        on: ProcessId,
        cap: Capability,
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    GetAll {
        on: ProcessId,
        responder: tokio::sync::oneshot::Sender<HashSet<SignedCapability>>,
    },
}

pub type ProcessMap = HashMap<ProcessId, PersistedProcess>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedProcess {
    pub wasm_bytes_handle: String,
    pub on_panic: OnPanic,
    pub capabilities: HashSet<Capability>,
    pub public: bool, // marks if a process allows messages from any process
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessContext {
    // store ultimate in order to set prompting message if needed
    pub prompting_message: Option<KernelMessage>,
    // can be empty if a request doesn't set context, but still needs to inherit
    pub context: Option<Context>,
}

pub type PackageVersion = (u32, u32, u32);

/// the type that gets deserialized from `metadata.json` in a package
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub package: String,
    pub publisher: String,
    pub version: PackageVersion,
    pub description: Option<String>,
    pub website: Option<String>,
}

/// the type that gets deserialized from each entry in the array in `manifest.json`
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageManifestEntry {
    pub process_name: String,
    pub process_wasm_path: String,
    pub on_panic: OnPanic,
    pub request_networking: bool,
    pub request_messaging: Option<Vec<serde_json::Value>>,
    pub grant_messaging: Option<Vec<serde_json::Value>>,
    pub public: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum StateAction {
    GetState(ProcessId),
    SetState(ProcessId),
    DeleteState(ProcessId),
    Backup,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum StateResponse {
    GetState,
    SetState,
    DeleteState,
    Backup,
    Err(StateError),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum StateError {
    #[error("kernel_state: rocksdb internal error: {error}")]
    RocksDBError { action: String, error: String },
    #[error("kernel_state: startup error")]
    StartupError { action: String },
    #[error("vfs: Bytes payload required for {action}")]
    BadBytes { action: String },
    #[error("kernel_state: bad request error: {error}")]
    BadRequest { error: String },
    #[error("kernel_state: Bad JSON payload: {error}")]
    BadJson { error: String },
    #[error("kernel_state: state not found for ProcessId {process_id}")]
    NotFound { process_id: ProcessId },
}

#[allow(dead_code)]
impl StateError {
    pub fn kind(&self) -> &str {
        match *self {
            StateError::RocksDBError { .. } => "RocksDBError",
            StateError::StartupError { .. } => "StartupError",
            StateError::BadBytes { .. } => "BadBytes",
            StateError::BadRequest { .. } => "BadRequest",
            StateError::BadJson { .. } => "NoJson",
            StateError::NotFound { .. } => "NotFound",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VfsRequest {
    pub path: String,
    pub action: VfsAction,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsAction {
    CreateDrive,
    CreateDir,
    CreateDirAll,
    CreateFile,
    OpenFile,
    CloseFile,
    WriteAll,
    Write,
    WriteAt(u64),
    Append,
    SyncAll,
    Read,
    ReadDir,
    ReadExact(u64),
    ReadToString,
    Seek(SeekFrom),
    RemoveFile,
    RemoveDir,
    RemoveDirAll,
    Rename(String),
    AddZip,
    // Metadata,
    Len,
    SetLen(u64),
    Hash,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VfsResponse {
    Ok,
    Err(VfsError),
    Read,
    ReadDir(Vec<String>),
    ReadToString(String),
    Len(u64),
    Hash([u8; 32]),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum VfsError {
    #[error("vfs: No capability for action {action} at path {path}")]
    NoCap { action: String, path: String },
    #[error("vfs: Bytes payload required for {action} at path {path}")]
    BadBytes { action: String, path: String },
    #[error("vfs: bad request error: {error}")]
    BadRequest { error: String },
    #[error("vfs: error parsing path: {path}, error: {error}")]
    ParseError { error: String, path: String },
    #[error("vfs: IO error: {error}, at path {path}")]
    IOError { error: String, path: String },
    #[error("vfs: kernel capability channel error: {error}")]
    CapChannelFail { error: String },
    #[error("vfs: Bad JSON payload: {error}")]
    BadJson { error: String },
    #[error("vfs: File not found at path {path}")]
    NotFound { path: String },
    #[error("vfs: Creating directory failed at path: {path}: {error}")]
    CreateDirError { path: String, error: String },
}

#[allow(dead_code)]
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
        }
    }
}
