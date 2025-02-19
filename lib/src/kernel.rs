use crate::types::core::{
    display_message, Address, Capability, LazyLoadBlob, Message, NodeId, OnExit, ProcessId,
    SendError,
};
use ring::signature;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;

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

pub type ProcessMessageSender = tokio::sync::mpsc::Sender<Result<KernelMessage, WrappedSendError>>;
pub type ProcessMessageReceiver =
    tokio::sync::mpsc::Receiver<Result<KernelMessage, WrappedSendError>>;

//
// types used for onchain identity system
//

#[derive(Debug)]
pub struct Keyfile {
    pub username: String,
    pub routers: Vec<String>,
    pub networking_keypair: signature::Ed25519KeyPair,
    pub jwt_secret_bytes: Vec<u8>,
    pub file_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootInfo {
    pub password_hash: String,
    pub username: String,
    pub reset: bool,
    pub direct: bool,
    pub owner: String,
    pub signature: String,
    pub timestamp: u64,
    pub chain_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportKeyfileInfo {
    pub password_hash: String,
    pub keyfile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginInfo {
    pub password_hash: String,
    pub subdomain: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub name: NodeId,
    pub networking_key: String,
    pub routing: NodeRouting,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeRouting {
    Routers(Vec<NodeId>),
    Direct {
        ip: String,
        ports: BTreeMap<String, u16>,
    },
    /// currently only used for initial registration...
    Both {
        ip: String,
        ports: BTreeMap<String, u16>,
        routers: Vec<NodeId>,
    },
}

impl Identity {
    pub fn is_direct(&self) -> bool {
        match &self.routing {
            NodeRouting::Direct { .. } => true,
            _ => false,
        }
    }
    pub fn get_protocol_port(&self, protocol: &str) -> Option<&u16> {
        match &self.routing {
            NodeRouting::Routers(_) => None,
            NodeRouting::Direct { ports, .. } | NodeRouting::Both { ports, .. } => {
                ports.get(protocol)
            }
        }
    }
    pub fn get_ip(&self) -> Option<&str> {
        match &self.routing {
            NodeRouting::Routers(_) => None,
            NodeRouting::Direct { ip, .. } | NodeRouting::Both { ip, .. } => Some(ip),
        }
    }
    pub fn ws_routing(&self) -> Option<(&str, &u16)> {
        match &self.routing {
            NodeRouting::Routers(_) => None,
            NodeRouting::Direct { ip, ports } | NodeRouting::Both { ip, ports, .. } => {
                if let Some(port) = ports.get("ws") {
                    if *port != 0 {
                        Some((ip, port))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
    pub fn tcp_routing(&self) -> Option<(&str, &u16)> {
        match &self.routing {
            NodeRouting::Routers(_) => None,
            NodeRouting::Direct { ip, ports } | NodeRouting::Both { ip, ports, .. } => {
                if let Some(port) = ports.get("tcp") {
                    if *port != 0 {
                        Some((ip, port))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
    }
    pub fn routers(&self) -> Option<&Vec<NodeId>> {
        match &self.routing {
            NodeRouting::Routers(routers) | NodeRouting::Both { routers, .. } => Some(routers),
            NodeRouting::Direct { .. } => None,
        }
    }
    pub fn both_to_direct(&mut self) {
        if let NodeRouting::Both {
            ip,
            ports,
            routers: _,
        } = self.routing.clone()
        {
            self.routing = NodeRouting::Direct { ip, ports };
        }
    }
    pub fn both_to_routers(&mut self) {
        if let NodeRouting::Both {
            ip: _,
            ports: _,
            routers,
        } = self.routing.clone()
        {
            self.routing = NodeRouting::Routers(routers);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnencryptedIdentity {
    pub name: NodeId,
    pub allowed_routers: Vec<NodeId>,
}

//
// kernel types that runtime modules use
//

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessMetadata {
    pub our: Address,
    pub wasm_bytes_handle: String,
    /// if None, use the oldest version: 0.7.0
    pub wit_version: Option<u32>,
    pub on_exit: OnExit,
    pub public: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelMessage {
    pub id: u64,
    pub source: Address,
    pub target: Address,
    pub rsvp: Rsvp,
    pub message: Message,
    pub lazy_load_blob: Option<LazyLoadBlob>,
}

impl KernelMessage {
    pub fn builder() -> KernelMessageBuilder {
        KernelMessageBuilder::default()
    }

    pub async fn send(self, sender: &MessageSender) {
        let Err(e) = sender.try_send(self) else {
            // not Err -> send successful; done here
            return;
        };
        // its an Err: handle
        match e {
            tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                panic!("kernel message sender: receiver closed");
            }
            tokio::sync::mpsc::error::TrySendError::Full(_) => {
                // TODO: implement backpressure
                panic!("kernel overloaded with messages: TODO: implement backpressure");
            }
        }
    }
}

#[derive(Default)]
pub struct KernelMessageBuilder {
    id: u64,
    source: Option<Address>,
    target: Option<Address>,
    rsvp: Rsvp,
    message: Option<Message>,
    lazy_load_blob: Option<LazyLoadBlob>,
}

impl KernelMessageBuilder {
    pub fn id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    pub fn source<T>(mut self, source: T) -> Self
    where
        T: Into<Address>,
    {
        self.source = Some(source.into());
        self
    }

    pub fn target<T>(mut self, target: T) -> Self
    where
        T: Into<Address>,
    {
        self.target = Some(target.into());
        self
    }

    pub fn rsvp(mut self, rsvp: Rsvp) -> Self {
        self.rsvp = rsvp;
        self
    }

    pub fn message(mut self, message: Message) -> Self {
        self.message = Some(message);
        self
    }

    pub fn lazy_load_blob(mut self, blob: Option<LazyLoadBlob>) -> Self {
        self.lazy_load_blob = blob;
        self
    }

    pub fn build(self) -> Result<KernelMessage, String> {
        Ok(KernelMessage {
            id: self.id,
            source: self.source.ok_or("Source address is required")?,
            target: self.target.ok_or("Target address is required")?,
            rsvp: self.rsvp,
            message: self.message.ok_or("Message is required")?,
            lazy_load_blob: self.lazy_load_blob,
        })
    }
}

impl std::fmt::Display for KernelMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{{\n    id: {},\n    source: {},\n    target: {},\n    rsvp: {},\n    message: {},\n    blob: {},\n}}",
            self.id,
            self.source,
            self.target,
            match &self.rsvp {
                Some(rsvp) => rsvp.to_string(),
                None => "None".to_string()
            },
            display_message(&self.message, "\n        "),
            self.lazy_load_blob.is_some(),
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
    pub source: ProcessId,
    pub content: String,
}

impl Printout {
    pub fn new<T, U>(verbosity: u8, source: T, content: U) -> Self
    where
        T: Into<ProcessId>,
        U: Into<String>,
    {
        Self {
            verbosity,
            source: source.into(),
            content: content.into(),
        }
    }

    /// Fire the printout to the terminal without checking for success.
    pub async fn send(self, sender: &PrintSender) {
        let _ = sender.send(self).await;
    }
}

#[derive(Error, Debug)]
pub enum ProcessVerbosityValError {
    #[error("Parse failed; must be `m` `mute` or `muted` (-> `Muted`) OR a u8")]
    ParseFailed,
}

#[derive(Clone, Deserialize, Serialize)]
pub enum ProcessVerbosityVal {
    U8(u8),
    Muted,
}

impl ProcessVerbosityVal {
    pub fn get_verbosity(&self) -> Option<&u8> {
        match self {
            ProcessVerbosityVal::U8(v) => Some(v),
            ProcessVerbosityVal::Muted => None,
        }
    }
}

impl std::str::FromStr for ProcessVerbosityVal {
    type Err = ProcessVerbosityValError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input == "m" || input == "mute" || input == "muted" {
            return Ok(Self::Muted);
        }
        let Ok(u) = input.parse::<u8>() else {
            return Err(ProcessVerbosityValError::ParseFailed);
        };
        Ok(Self::U8(u))
    }
}

impl std::fmt::Display for ProcessVerbosityVal {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ProcessVerbosityVal::U8(verbosity) => format!("{verbosity}"),
                ProcessVerbosityVal::Muted => "muted".to_string(),
            },
        )
    }
}

pub type ProcessVerbosity = HashMap<ProcessId, ProcessVerbosityVal>;

/// kernel sets in case, e.g.,
///  A requests response from B does not request response from C
///  -> kernel sets `Some(A) = Rsvp` for B's request to C
pub type Rsvp = Option<Address>;

#[derive(Debug, Serialize, Deserialize)]
pub enum DebugCommand {
    ToggleStepthrough,
    Step,
    ToggleEventLoop,
    ToggleEventLoopForProcess(ProcessId),
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
    ///
    /// All capabilities passed into initial_capabilities must be held by the source
    /// of this message, or the kernel will discard them (silently for now).
    InitializeProcess {
        id: ProcessId,
        wasm_bytes_handle: String,
        wit_version: Option<u32>,
        on_exit: OnExit,
        initial_capabilities: HashSet<Capability>,
        public: bool,
    },
    /// Create an arbitrary capability and grant it to a process.
    GrantCapabilities {
        target: ProcessId,
        capabilities: Vec<Capability>,
    },
    /// Drop capabilities. Does nothing if process doesn't have these caps
    DropCapabilities {
        target: ProcessId,
        capabilities: Vec<Capability>,
    },
    /// Set the on-exit behavior for a process.
    SetOnExit { target: ProcessId, on_exit: OnExit },
    /// Tell the kernel to run a process that has already been installed.
    /// TODO: in the future, this command could be extended to allow for
    /// resource provision.
    RunProcess(ProcessId),
    /// Kill a running process immediately. This may result in the dropping / mishandling of messages!
    KillProcess(ProcessId),
    /// RUNTIME ONLY: notify the kernel that the runtime is shutting down and it
    /// should gracefully stop and persist the running processes.
    Shutdown,
    /// Ask kernel to produce debugging information
    Debug(KernelPrint),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KernelPrint {
    ProcessMap,
    Process(ProcessId),
    HasCap { on: ProcessId, cap: Capability },
}

/// IPC format for all KernelCommand responses
#[derive(Debug, Serialize, Deserialize)]
pub enum KernelResponse {
    InitializedProcess,
    InitializeProcessError,
    StartedProcess,
    RunProcessError,
    KilledProcess(ProcessId),
    Debug(KernelPrintResponse),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KernelPrintResponse {
    ProcessMap(UserspaceProcessMap),
    Process(Option<UserspacePersistedProcess>),
    HasCap(Option<bool>),
}

#[derive(Debug)]
pub enum CapMessage {
    /// root access: uncritically sign and add all `caps` to `on`
    Add {
        on: ProcessId,
        caps: Vec<Capability>,
        responder: Option<tokio::sync::oneshot::Sender<bool>>,
    },
    /// root delete: uncritically remove all `caps` from `on`
    Drop {
        on: ProcessId,
        caps: Vec<Capability>,
        responder: Option<tokio::sync::oneshot::Sender<bool>>,
    },
    /// does `on` have `cap` in its store?
    Has {
        // a bool is given in response here
        on: ProcessId,
        cap: Capability,
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    /// return all caps in `on`'s store
    GetAll {
        on: ProcessId,
        responder: tokio::sync::oneshot::Sender<Vec<(Capability, Vec<u8>)>>,
    },
    /// Remove all caps issued by `on` from every process on the entire system
    RevokeAll {
        on: ProcessId,
        responder: Option<tokio::sync::oneshot::Sender<bool>>,
    },
    /// before `on` sends a message, filter out any bogus caps it may have attached, sign any new
    /// caps it may have created, and retreive the signature for the caps in its store.
    FilterCaps {
        on: ProcessId,
        caps: Vec<Capability>,
        responder: tokio::sync::oneshot::Sender<Vec<(Capability, Vec<u8>)>>,
    },
}

impl std::fmt::Display for CapMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CapMessage::Add { on, caps, .. } => write!(
                f,
                "caps: add {} on {on}",
                caps.iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            CapMessage::Drop { on, caps, .. } => write!(
                f,
                "caps: drop {} on {on}",
                caps.iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
            CapMessage::Has { on, cap, .. } => write!(f, "caps: has {} on {on}", cap),
            CapMessage::GetAll { on, .. } => write!(f, "caps: get all on {on}"),
            CapMessage::RevokeAll { on, .. } => write!(f, "caps: revoke all on {on}"),
            CapMessage::FilterCaps { on, caps, .. } => {
                write!(
                    f,
                    "caps: filter for {} on {on}",
                    caps.iter()
                        .map(|c| c.to_string())
                        .collect::<Vec<String>>()
                        .join(", ")
                )
            }
        }
    }
}

pub type ReverseCapIndex = HashMap<ProcessId, HashMap<ProcessId, Vec<Capability>>>;

pub type ProcessMap = HashMap<ProcessId, PersistedProcess>;
pub type UserspaceProcessMap = HashMap<ProcessId, UserspacePersistedProcess>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedProcess {
    pub wasm_bytes_handle: String,
    pub wit_version: Option<u32>,
    pub on_exit: OnExit,
    pub capabilities: HashMap<Capability, Vec<u8>>,
    /// marks if a process allows messages from any process
    pub public: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserspacePersistedProcess {
    pub wasm_bytes_handle: String,
    pub wit_version: Option<u32>,
    pub on_exit: OnExit,
    pub capabilities: HashSet<Capability>,
    pub public: bool,
}

impl From<PersistedProcess> for UserspacePersistedProcess {
    fn from(p: PersistedProcess) -> Self {
        UserspacePersistedProcess {
            wasm_bytes_handle: p.wasm_bytes_handle,
            wit_version: p.wit_version,
            on_exit: p.on_exit,
            capabilities: p.capabilities.into_keys().collect(),
            public: p.public,
        }
    }
}

/// Represents the metadata associated with a hyperware package, which is an ERC721 compatible token.
/// This is deserialized from the `metadata.json` file in a package.
/// Fields:
/// - `name`: An optional field representing the display name of the package. This does not have to be unique, and is not used for identification purposes.
/// - `description`: An optional field providing a description of the package.
/// - `image`: An optional field containing a URL to an image representing the package.
/// - `external_url`: An optional field containing a URL for more information about the package. For example, a link to the github repository.
/// - `animation_url`: An optional field containing a URL to an animation or video representing the package.
/// - `properties`: A requried field containing important information about the package.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Erc721Metadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub external_url: Option<String>,
    pub animation_url: Option<String>,
    pub properties: Erc721Properties,
}

/// Represents critical fields of a hyperware package in an ERC721 compatible format.
/// This follows the [ERC1155](https://github.com/ethereum/ercs/blob/master/ERCS/erc-1155.md#erc-1155-metadata-uri-json-schema) metadata standard.
///
/// Fields:
/// - `package_name`: The unique name of the package, used in the `PackageId`, e.g. `package_name:publisher`.
/// - `publisher`: The HNS identity of the package publisher used in the `PackageId`, e.g. `package_name:publisher`
/// - `current_version`: A string representing the current version of the package, e.g. `1.0.0`.
/// - `mirrors`: A list of NodeIds where the package can be found, providing redundancy.
/// - `code_hashes`: A map from version names to their respective SHA-256 hashes.
/// - `license`: An optional field containing the license of the package.
/// - `screenshots`: An optional field containing a list of URLs to screenshots of the package.
/// - `wit_version`: An optional field containing the version of the WIT standard that the package adheres to.
/// - `dependencies`: An optional field containing a list of `PackageId`s: API dependencies.
/// - `api_includes`: An optional field containing a list of `PathBuf`s: additional files to include in the `api.zip`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Erc721Properties {
    pub package_name: String,
    pub publisher: String,
    pub current_version: String,
    pub mirrors: Vec<NodeId>,
    pub code_hashes: HashMap<String, String>,
    pub license: Option<String>,
    pub screenshots: Option<Vec<String>>,
    pub wit_version: Option<u32>,
    pub dependencies: Option<Vec<String>>,
    pub api_includes: Option<Vec<std::path::PathBuf>>,
}

/// the type that gets deserialized from each entry in the array in `manifest.json`
#[derive(Debug, Serialize, Deserialize)]
pub struct PackageManifestEntry {
    pub process_name: String,
    pub process_wasm_path: String,
    pub on_exit: OnExit,
    pub request_networking: bool,
    pub request_capabilities: Vec<serde_json::Value>,
    pub grant_capabilities: Vec<serde_json::Value>,
    pub public: bool,
}
