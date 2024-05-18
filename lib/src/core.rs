use crate::wit;
use ring::signature;
use rusqlite::types::{FromSql, FromSqlError, ToSql, ValueRef};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, BTreeMap};
use thiserror::Error;

lazy_static::lazy_static! {
    pub static ref ETH_PROCESS_ID: ProcessId = ProcessId::new(Some("eth"), "distro", "sys");
    pub static ref HTTP_CLIENT_PROCESS_ID: ProcessId = ProcessId::new(Some("http_client"), "distro", "sys");
    pub static ref HTTP_SERVER_PROCESS_ID: ProcessId = ProcessId::new(Some("http_server"), "distro", "sys");
    pub static ref KERNEL_PROCESS_ID: ProcessId = ProcessId::new(Some("kernel"), "distro", "sys");
    pub static ref TERMINAL_PROCESS_ID: ProcessId = ProcessId::new(Some("terminal"), "terminal", "sys");
    pub static ref TIMER_PROCESS_ID: ProcessId = ProcessId::new(Some("timer"), "distro", "sys");
    pub static ref VFS_PROCESS_ID: ProcessId = ProcessId::new(Some("vfs"), "distro", "sys");
    pub static ref STATE_PROCESS_ID: ProcessId = ProcessId::new(Some("state"), "distro", "sys");
    pub static ref KV_PROCESS_ID: ProcessId = ProcessId::new(Some("kv"), "distro", "sys");
    pub static ref SQLITE_PROCESS_ID: ProcessId = ProcessId::new(Some("sqlite"), "distro", "sys");
}

//
// types shared between kernel and processes. frustratingly, this is an exact copy
// of the types in process_lib
// this is because even though the types are identical, they will not match when
// used in the kernel context which generates bindings differently than the process
// standard library. make sure to keep this synced with process_lib.
//
pub type Context = Vec<u8>;
pub type NodeId = String; // KNS domain name

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
        s.parse().map_err(serde::de::Error::custom)
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
    pub fn _package(&self) -> &str {
        &self.package_name
    }
    pub fn _publisher(&self) -> &str {
        &self.publisher_node
    }
}

impl std::str::FromStr for PackageId {
    type Err = ProcessIdParseError;
    /// Attempt to parse a `PackageId` from a string. The string must
    /// contain exactly two segments, where segments are non-empty strings
    /// separated by a colon (`:`). The segments cannot themselves contain colons.
    ///
    /// Please note that while any string without colons will parse successfully
    /// to create a `PackageId`, not all strings without colons are actually
    /// valid usernames, which the `publisher_node` field of a `PackageId` will
    /// always in practice be.
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let segments: Vec<&str> = input.split(':').collect();
        if segments.len() < 2 {
            return Err(ProcessIdParseError::MissingField);
        } else if segments.len() > 2 {
            return Err(ProcessIdParseError::TooManyColons);
        }
        let package_name = segments[0].to_string();
        if package_name.is_empty() {
            return Err(ProcessIdParseError::MissingField);
        }
        let publisher_node = segments[1].to_string();
        if publisher_node.is_empty() {
            return Err(ProcessIdParseError::MissingField);
        }

        Ok(PackageId {
            package_name,
            publisher_node,
        })
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
    pub fn en_wit_v0(&self) -> crate::v0::wit::ProcessId {
        crate::v0::wit::ProcessId {
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
    pub fn de_wit_v0(wit: crate::v0::wit::ProcessId) -> ProcessId {
        ProcessId {
            process_name: wit.process_name,
            package_name: wit.package_name,
            publisher_node: wit.publisher_node,
        }
    }
}

impl std::str::FromStr for ProcessId {
    type Err = ProcessIdParseError;
    /// Attempts to parse a `ProcessId` from a string. To succeed, the string must contain
    /// exactly 3 segments, separated by colons `:`. The segments must not contain colons.
    /// Please note that while any string without colons will parse successfully
    /// to create a `ProcessId`, not all strings without colons are actually
    /// valid usernames, which the `publisher_node` field of a `ProcessId` will
    /// always in practice be.
    fn from_str(input: &str) -> Result<Self, ProcessIdParseError> {
        let segments: Vec<&str> = input.split(':').collect();
        if segments.len() < 3 {
            return Err(ProcessIdParseError::MissingField);
        } else if segments.len() > 3 {
            return Err(ProcessIdParseError::TooManyColons);
        }
        let process_name = segments[0].to_string();
        if process_name.is_empty() {
            return Err(ProcessIdParseError::MissingField);
        }
        let package_name = segments[1].to_string();
        if package_name.is_empty() {
            return Err(ProcessIdParseError::MissingField);
        }
        let publisher_node = segments[2].to_string();
        if publisher_node.is_empty() {
            return Err(ProcessIdParseError::MissingField);
        }
        Ok(ProcessId {
            process_name,
            package_name,
            publisher_node,
        })
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
        write!(f, "{}", self.to_string())
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
    pub fn en_wit(&self) -> wit::Address {
        wit::Address {
            node: self.node.clone(),
            process: self.process.en_wit(),
        }
    }
    pub fn en_wit_v0(&self) -> crate::v0::wit::Address {
        crate::v0::wit::Address {
            node: self.node.clone(),
            process: self.process.en_wit_v0(),
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
    pub fn de_wit_v0(wit: crate::v0::wit::Address) -> Address {
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

impl std::str::FromStr for Address {
    type Err = AddressParseError;
    /// Attempt to parse an `Address` from a string. The formatting structure for
    /// an Address is `node@process_name:package_name:publisher_node`.
    ///
    /// The string being parsed must contain exactly one `@` and three `:` characters.
    /// The `@` character separates the node ID from the rest of the address, and the
    /// `:` characters separate the process name, package name, and publisher node ID.
    fn from_str(input: &str) -> Result<Self, AddressParseError> {
        // split string on '@' and ensure there is exactly one '@'
        let parts: Vec<&str> = input.split('@').collect();
        if parts.len() < 2 {
            return Err(AddressParseError::MissingNodeId);
        } else if parts.len() > 2 {
            return Err(AddressParseError::TooManyAts);
        }
        let node = parts[0].to_string();
        if node.is_empty() {
            return Err(AddressParseError::MissingNodeId);
        }

        // split the rest on ':' and ensure there are exactly three ':'
        let segments: Vec<&str> = parts[1].split(':').collect();
        if segments.len() < 3 {
            return Err(AddressParseError::MissingField);
        } else if segments.len() > 3 {
            return Err(AddressParseError::TooManyColons);
        }
        let process_name = segments[0].to_string();
        if process_name.is_empty() {
            return Err(AddressParseError::MissingField);
        }
        let package_name = segments[1].to_string();
        if package_name.is_empty() {
            return Err(AddressParseError::MissingField);
        }
        let publisher_node = segments[2].to_string();
        if publisher_node.is_empty() {
            return Err(AddressParseError::MissingField);
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
        s.parse().map_err(serde::de::Error::custom)
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
pub enum AddressParseError {
    TooManyAts,
    TooManyColons,
    MissingNodeId,
    MissingField,
}

impl std::fmt::Display for AddressParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl std::error::Error for AddressParseError {
    fn description(&self) -> &str {
        match self {
            AddressParseError::TooManyAts => "Too many '@' chars in ProcessId string",
            AddressParseError::TooManyColons => "Too many colons in ProcessId string",
            AddressParseError::MissingNodeId => "Node ID missing",
            AddressParseError::MissingField => "Missing field in ProcessId string",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LazyLoadBlob {
    pub mime: Option<String>, // MIME type
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub inherit: bool,
    pub expects_response: Option<u64>, // number of seconds until timeout
    pub body: Vec<u8>,
    pub metadata: Option<String>, // JSON-string
    pub capabilities: Vec<(Capability, Vec<u8>)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub inherit: bool,
    pub body: Vec<u8>,
    pub metadata: Option<String>, // JSON-string
    pub capabilities: Vec<(Capability, Vec<u8>)>,
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

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({})",
            self.issuer,
            serde_json::from_str::<serde_json::Value>(&self.params)
                .unwrap_or(serde_json::json!("invalid JSON in capability"))
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendError {
    pub kind: SendErrorKind,
    pub target: Address,
    pub message: Message,
    pub lazy_load_blob: Option<LazyLoadBlob>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SendErrorKind {
    Offline,
    Timeout,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OnExit {
    None,
    Restart,
    Requests(Vec<(Address, Request, Option<LazyLoadBlob>)>),
}

impl OnExit {
    pub fn is_restart(&self) -> bool {
        match self {
            OnExit::None => false,
            OnExit::Restart => true,
            OnExit::Requests(_) => false,
        }
    }

    pub fn is_none(&self) -> bool {
        match self {
            OnExit::None => true,
            OnExit::Restart => false,
            OnExit::Requests(_) => false,
        }
    }

    pub fn en_wit(&self) -> wit::OnExit {
        match self {
            OnExit::None => wit::OnExit::None,
            OnExit::Restart => wit::OnExit::Restart,
            OnExit::Requests(reqs) => wit::OnExit::Requests(
                reqs.iter()
                    .map(|(address, request, blob)| {
                        (
                            address.en_wit(),
                            en_wit_request(request.clone()),
                            en_wit_blob(blob.clone()),
                        )
                    })
                    .collect(),
            ),
        }
    }

    pub fn en_wit_v0(&self) -> crate::v0::wit::OnExit {
        match self {
            OnExit::None => crate::v0::wit::OnExit::None,
            OnExit::Restart => crate::v0::wit::OnExit::Restart,
            OnExit::Requests(reqs) => crate::v0::wit::OnExit::Requests(
                reqs.iter()
                    .map(|(address, request, blob)| {
                        (
                            address.en_wit_v0(),
                            en_wit_request_v0(request.clone()),
                            en_wit_blob_v0(blob.clone()),
                        )
                    })
                    .collect(),
            ),
        }
    }

    pub fn de_wit(wit: wit::OnExit) -> Self {
        match wit {
            wit::OnExit::None => OnExit::None,
            wit::OnExit::Restart => OnExit::Restart,
            wit::OnExit::Requests(reqs) => OnExit::Requests(
                reqs.into_iter()
                    .map(|(address, request, blob)| {
                        (
                            Address::de_wit(address),
                            de_wit_request(request),
                            de_wit_blob(blob),
                        )
                    })
                    .collect(),
            ),
        }
    }

    pub fn de_wit_v0(wit: crate::v0::wit::OnExit) -> Self {
        match wit {
            crate::v0::wit::OnExit::None => OnExit::None,
            crate::v0::wit::OnExit::Restart => OnExit::Restart,
            crate::v0::wit::OnExit::Requests(reqs) => OnExit::Requests(
                reqs.into_iter()
                    .map(|(address, request, blob)| {
                        (
                            Address::de_wit_v0(address),
                            de_wit_request_v0(request),
                            de_wit_blob_v0(blob),
                        )
                    })
                    .collect(),
            ),
        }
    }
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", display_message(self, "\n    "))
    }
}

fn display_capabilities(capabilities: &Vec<(Capability, Vec<u8>)>, delimiter: &str) -> String {
    if capabilities.is_empty() {
        return "[],".to_string();
    }

    let mut caps_string = "[".to_string();
    for cap in capabilities.iter() {
        caps_string += &format!("{}    {}", delimiter, cap.0.to_string());
    }
    format!("{}{}]", caps_string, delimiter)
}

fn display_message(m: &Message, delimiter: &str) -> String {
    let lines = match m {
        Message::Request(request) => {
            vec![
                "Request(".into(),
                format!("inherit: {},", request.inherit),
                format!("expects_response: {:?},", request.expects_response),
                format!(
                    "body: {},",
                    match std::str::from_utf8(&request.body) {
                        Ok(str) => str.to_string(),
                        Err(_) => format!("{:?}", request.body),
                    }
                ),
                format!(
                    "metadata: {},",
                    &request.metadata.as_ref().unwrap_or(&"None".into())
                ),
                format!(
                    "capabilities: {}",
                    display_capabilities(&request.capabilities, delimiter)
                ),
            ]
        }
        Message::Response((response, context)) => {
            vec![
                "Response(".into(),
                format!("inherit: {},", response.inherit),
                format!(
                    "body: {},",
                    match serde_json::from_slice::<serde_json::Value>(&response.body) {
                        Ok(json) => format!("{}", json),
                        Err(_) => format!("{:?}", response.body),
                    }
                ),
                format!(
                    "metadata: {},",
                    &response.metadata.as_ref().unwrap_or(&"None".into())
                ),
                format!(
                    "context: {},",
                    if context.is_none() {
                        "None".into()
                    } else {
                        match serde_json::from_slice::<serde_json::Value>(context.as_ref().unwrap())
                        {
                            Ok(json) => format!("{}", json),
                            Err(_) => format!("{:?}", context.as_ref().unwrap()),
                        }
                    },
                ),
                format!(
                    "capabilities: {}",
                    display_capabilities(&response.capabilities, delimiter)
                ),
            ]
        }
    };
    lines.into_iter().collect::<Vec<_>>().join(delimiter) + &delimiter[..delimiter.len() - 4] + ")"
}

//
// conversions between wit types and kernel types (annoying!)
//

pub fn de_wit_request(wit: wit::Request) -> Request {
    Request {
        inherit: wit.inherit,
        expects_response: wit.expects_response,
        body: wit.body,
        metadata: wit.metadata,
        capabilities: wit
            .capabilities
            .iter()
            .map(|cap| de_wit_capability(cap.clone()))
            .collect(),
    }
}

pub fn de_wit_request_v0(wit: crate::v0::wit::Request) -> Request {
    Request {
        inherit: wit.inherit,
        expects_response: wit.expects_response,
        body: wit.body,
        metadata: wit.metadata,
        capabilities: wit
            .capabilities
            .iter()
            .map(|cap| de_wit_capability_v0(cap.clone()))
            .collect(),
    }
}

pub fn en_wit_request(request: Request) -> wit::Request {
    wit::Request {
        inherit: request.inherit,
        expects_response: request.expects_response,
        body: request.body,
        metadata: request.metadata,
        capabilities: request
            .capabilities
            .iter()
            .map(|cap| en_wit_capability(cap.clone()))
            .collect(),
    }
}

pub fn en_wit_request_v0(request: Request) -> crate::v0::wit::Request {
    crate::v0::wit::Request {
        inherit: request.inherit,
        expects_response: request.expects_response,
        body: request.body,
        metadata: request.metadata,
        capabilities: request
            .capabilities
            .iter()
            .map(|cap| en_wit_capability_v0(cap.clone()))
            .collect(),
    }
}

pub fn de_wit_response(wit: wit::Response) -> Response {
    Response {
        inherit: wit.inherit,
        body: wit.body,
        metadata: wit.metadata,
        capabilities: wit
            .capabilities
            .iter()
            .map(|cap| de_wit_capability(cap.clone()))
            .collect(),
    }
}

pub fn de_wit_response_v0(wit: crate::v0::wit::Response) -> Response {
    Response {
        inherit: wit.inherit,
        body: wit.body,
        metadata: wit.metadata,
        capabilities: wit
            .capabilities
            .iter()
            .map(|cap| de_wit_capability_v0(cap.clone()))
            .collect(),
    }
}

pub fn en_wit_response(response: Response) -> wit::Response {
    wit::Response {
        inherit: response.inherit,
        body: response.body,
        metadata: response.metadata,
        capabilities: response
            .capabilities
            .iter()
            .map(|cap| en_wit_capability(cap.clone()))
            .collect(),
    }
}

pub fn en_wit_response_v0(response: Response) -> crate::v0::wit::Response {
    crate::v0::wit::Response {
        inherit: response.inherit,
        body: response.body,
        metadata: response.metadata,
        capabilities: response
            .capabilities
            .iter()
            .map(|cap| en_wit_capability_v0(cap.clone()))
            .collect(),
    }
}

pub fn de_wit_blob(wit: Option<wit::LazyLoadBlob>) -> Option<LazyLoadBlob> {
    match wit {
        None => None,
        Some(wit) => Some(LazyLoadBlob {
            mime: wit.mime,
            bytes: wit.bytes,
        }),
    }
}

pub fn de_wit_blob_v0(wit: Option<crate::v0::wit::LazyLoadBlob>) -> Option<LazyLoadBlob> {
    match wit {
        None => None,
        Some(wit) => Some(LazyLoadBlob {
            mime: wit.mime,
            bytes: wit.bytes,
        }),
    }
}

pub fn en_wit_blob(load: Option<LazyLoadBlob>) -> Option<wit::LazyLoadBlob> {
    match load {
        None => None,
        Some(load) => Some(wit::LazyLoadBlob {
            mime: load.mime,
            bytes: load.bytes,
        }),
    }
}

pub fn en_wit_blob_v0(load: Option<LazyLoadBlob>) -> Option<crate::v0::wit::LazyLoadBlob> {
    match load {
        None => None,
        Some(load) => Some(crate::v0::wit::LazyLoadBlob {
            mime: load.mime,
            bytes: load.bytes,
        }),
    }
}

pub fn de_wit_capability(wit: wit::Capability) -> (Capability, Vec<u8>) {
    (
        Capability {
            issuer: Address {
                node: wit.issuer.node,
                process: ProcessId {
                    process_name: wit.issuer.process.process_name,
                    package_name: wit.issuer.process.package_name,
                    publisher_node: wit.issuer.process.publisher_node,
                },
            },
            params: wit.params,
        },
        vec![],
    )
}

pub fn de_wit_capability_v0(wit: crate::v0::wit::Capability) -> (Capability, Vec<u8>) {
    (
        Capability {
            issuer: Address {
                node: wit.issuer.node,
                process: ProcessId {
                    process_name: wit.issuer.process.process_name,
                    package_name: wit.issuer.process.package_name,
                    publisher_node: wit.issuer.process.publisher_node,
                },
            },
            params: wit.params,
        },
        vec![],
    )
}

pub fn en_wit_capability(cap: (Capability, Vec<u8>)) -> wit::Capability {
    wit::Capability {
        issuer: cap.0.issuer.en_wit(),
        params: cap.0.params,
    }
}

pub fn en_wit_capability_v0(cap: (Capability, Vec<u8>)) -> crate::v0::wit::Capability {
    crate::v0::wit::Capability {
        issuer: cap.0.issuer.en_wit_v0(),
        params: cap.0.params,
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

pub fn en_wit_message_v0(message: Message) -> crate::v0::wit::Message {
    match message {
        Message::Request(request) => crate::v0::wit::Message::Request(en_wit_request_v0(request)),
        Message::Response((response, context)) => {
            crate::v0::wit::Message::Response((en_wit_response_v0(response), context))
        }
    }
}

pub fn en_wit_send_error(error: SendError) -> wit::SendError {
    wit::SendError {
        kind: en_wit_send_error_kind(error.kind),
        message: en_wit_message(error.message),
        lazy_load_blob: en_wit_blob(error.lazy_load_blob),
    }
}

pub fn en_wit_send_error_v0(error: SendError) -> crate::v0::wit::SendError {
    crate::v0::wit::SendError {
        kind: en_wit_send_error_kind_v0(error.kind),
        target: error.target.en_wit_v0(),
        message: en_wit_message_v0(error.message),
        lazy_load_blob: en_wit_blob_v0(error.lazy_load_blob),
    }
}

pub fn en_wit_send_error_kind(kind: SendErrorKind) -> wit::SendErrorKind {
    match kind {
        SendErrorKind::Offline => wit::SendErrorKind::Offline,
        SendErrorKind::Timeout => wit::SendErrorKind::Timeout,
    }
}

pub fn en_wit_send_error_kind_v0(kind: SendErrorKind) -> crate::v0::wit::SendErrorKind {
    match kind {
        SendErrorKind::Offline => crate::v0::wit::SendErrorKind::Offline,
        SendErrorKind::Timeout => crate::v0::wit::SendErrorKind::Timeout,
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
pub struct KeyfileVet {
    pub password_hash: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginAndResetInfo {
    pub password_hash: String,
    pub direct: bool,
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
        ws_port: u16,
        tcp_port: u16,
    },
    /// currently only used for initial registration...
    Both {
        ip: String,
        ws_port: u16,
        tcp_port: u16,
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
    pub fn ws_routing(&self) -> Option<(&str, &u16)> {
        match &self.routing {
            NodeRouting::Routers(_) => None,
            NodeRouting::Direct { ip, ws_port, .. } => Some((ip, ws_port)),
            NodeRouting::Both { ip, ws_port, .. } => Some((ip, ws_port)),
        }
    }
    pub fn routers(&self) -> Option<&Vec<NodeId>> {
        match &self.routing {
            NodeRouting::Routers(routers) => Some(routers),
            NodeRouting::Direct { .. } => None,
            NodeRouting::Both { routers, .. } => Some(routers),
        }
    }
    pub fn both_to_direct(&mut self) {
        if let NodeRouting::Both {
            ip,
            ws_port,
            tcp_port,
            routers: _,
        } = self.routing.clone()
        {
            self.routing = NodeRouting::Direct {
                ip,
                ws_port,
                tcp_port,
            };
        }
    }
    pub fn both_to_routers(&mut self) {
        if let NodeRouting::Both {
            ip: _,
            ws_port: _,
            tcp_port: _,
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
}

#[derive(Debug)]
pub enum CapMessage {
    /// root access: uncritically sign and add all `caps` to `on`
    Add {
        on: ProcessId,
        caps: Vec<Capability>,
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    /// root delete: uncritically remove all `caps` from `on`
    Drop {
        on: ProcessId,
        caps: Vec<Capability>,
        responder: tokio::sync::oneshot::Sender<bool>,
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
        responder: tokio::sync::oneshot::Sender<bool>,
    },
    /// before `on` sends a message, filter out any bogus caps it may have attached, sign any new
    /// caps it may have created, and retreive the signature for the caps in its store.
    FilterCaps {
        on: ProcessId,
        caps: Vec<Capability>,
        responder: tokio::sync::oneshot::Sender<Vec<(Capability, Vec<u8>)>>,
    },
}

pub type ReverseCapIndex = HashMap<ProcessId, HashMap<ProcessId, Vec<Capability>>>;

pub type ProcessMap = HashMap<ProcessId, PersistedProcess>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedProcess {
    pub wasm_bytes_handle: String,
    pub wit_version: Option<u32>,
    pub on_exit: OnExit,
    pub capabilities: HashMap<Capability, Vec<u8>>,
    pub public: bool, // marks if a process allows messages from any process
}

impl std::fmt::Display for PersistedProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Process {{\n    wasm_bytes_handle: {},\n    wit_version: {:?},\n    on_exit: {:?},\n    public: {}\n    capabilities: {}\n}}",
            {
                if &self.wasm_bytes_handle == "" {
                    "(none, this is a runtime process)"
                } else {
                    &self.wasm_bytes_handle
                }
            },
            self.wit_version,
            self.on_exit,
            self.public,
            {
                let mut caps_string = "[".to_string();
                for cap in self.capabilities.keys() {
                    caps_string += &format!("\n        {}", cap.to_string());
                }
                caps_string + "\n    ]"
            },
        )
    }
}

/// Represents the metadata associated with a kinode package, which is an ERC721 compatible token.
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

/// Represents critical fields of a kinode package in an ERC721 compatible format.
/// This follows the [ERC1155](https://github.com/ethereum/ercs/blob/master/ERCS/erc-1155.md#erc-1155-metadata-uri-json-schema) metadata standard.
///
/// Fields:
/// - `package_name`: The unique name of the package, used in the `PackageId`, e.g. `package_name:publisher`.
/// - `publisher`: The KNS identity of the package publisher used in the `PackageId`, e.g. `package_name:publisher`
/// - `current_version`: A string representing the current version of the package, e.g. `1.0.0`.
/// - `mirrors`: A list of NodeIds where the package can be found, providing redundancy.
/// - `code_hashes`: A map from version names to their respective SHA-256 hashes.
/// - `license`: An optional field containing the license of the package.
/// - `screenshots`: An optional field containing a list of URLs to screenshots of the package.
/// - `wit_version`: An optional field containing the version of the WIT standard that the package adheres to.
/// - `dependencies`: An optional field containing a list of `PackageId`s: API dependencies.
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
    #[error("kernel_state: bytes blob required for {action}")]
    BadBytes { action: String },
    #[error("kernel_state: bad request error: {error}")]
    BadRequest { error: String },
    #[error("kernel_state: Bad JSON blob: {error}")]
    BadJson { error: String },
    #[error("kernel_state: state not found for ProcessId {process_id}")]
    NotFound { process_id: ProcessId },
    #[error("kernel_state: IO error: {error}")]
    IOError { error: String },
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
            StateError::IOError { .. } => "IOError",
        }
    }
}

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
    ReadExact(u64),
    ReadToString,
    Seek { seek_from: SeekFrom },
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
    SeekFrom(u64),
    ReadDir(Vec<DirEntry>),
    ReadToString(String),
    Metadata(FileMetadata),
    Len(u64),
    Hash([u8; 32]),
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum VfsError {
    #[error("vfs: No capability for action {action} at path {path}")]
    NoCap { action: String, path: String },
    #[error("vfs: Bytes blob required for {action} at path {path}")]
    BadBytes { action: String, path: String },
    #[error("vfs: bad request error: {error}")]
    BadRequest { error: String },
    #[error("vfs: error parsing path: {path}, error: {error}")]
    ParseError { error: String, path: String },
    #[error("vfs: IO error: {error}, at path {path}")]
    IOError { error: String, path: String },
    #[error("vfs: kernel capability channel error: {error}")]
    CapChannelFail { error: String },
    #[error("vfs: Bad JSON blob: {error}")]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct KvRequest {
    pub package_id: PackageId,
    pub db: String,
    pub action: KvAction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum KvAction {
    Open,
    RemoveDb,
    Set { key: Vec<u8>, tx_id: Option<u64> },
    Delete { key: Vec<u8>, tx_id: Option<u64> },
    Get { key: Vec<u8> },
    BeginTx,
    Commit { tx_id: u64 },
    Backup,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KvResponse {
    Ok,
    BeginTx { tx_id: u64 },
    Get { key: Vec<u8> },
    Err { error: KvError },
}

#[derive(Debug, Serialize, Deserialize, Error)]
pub enum KvError {
    #[error("kv: DbDoesNotExist")]
    NoDb,
    #[error("kv: KeyNotFound")]
    KeyNotFound,
    #[error("kv: no Tx found")]
    NoTx,
    #[error("kv: No capability: {error}")]
    NoCap { error: String },
    #[error("kv: rocksdb internal error: {error}")]
    RocksDBError { action: String, error: String },
    #[error("kv: input bytes/json/key error: {error}")]
    InputError { error: String },
    #[error("kv: IO error: {error}")]
    IOError { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SqliteRequest {
    pub package_id: PackageId,
    pub db: String,
    pub action: SqliteAction,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SqliteAction {
    Open,
    RemoveDb,
    Write {
        statement: String,
        tx_id: Option<u64>,
    },
    Read {
        query: String,
    },
    BeginTx,
    Commit {
        tx_id: u64,
    },
    Backup,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SqliteResponse {
    Ok,
    Read,
    BeginTx { tx_id: u64 },
    Err { error: SqliteError },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SqlValue {
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Boolean(bool),
    Null,
}

#[derive(Debug, Serialize, Deserialize, Error)]
pub enum SqliteError {
    #[error("sqlite: DbDoesNotExist")]
    NoDb,
    #[error("sqlite: NoTx")]
    NoTx,
    #[error("sqlite: No capability: {error}")]
    NoCap { error: String },
    #[error("sqlite: UnexpectedResponse")]
    UnexpectedResponse,
    #[error("sqlite: NotAWriteKeyword")]
    NotAWriteKeyword,
    #[error("sqlite: NotAReadKeyword")]
    NotAReadKeyword,
    #[error("sqlite: Invalid Parameters")]
    InvalidParameters,
    #[error("sqlite: IO error: {error}")]
    IOError { error: String },
    #[error("sqlite: rusqlite error: {error}")]
    RusqliteError { error: String },
    #[error("sqlite: input bytes/json/key error: {error}")]
    InputError { error: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MessageType {
    Request,
    Response,
}

impl std::fmt::Display for KvAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for KvError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        KvError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for KvError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        KvError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<std::io::Error> for KvError {
    fn from(err: std::io::Error) -> Self {
        KvError::IOError {
            error: err.to_string(),
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

impl From<std::io::Error> for StateError {
    fn from(err: std::io::Error) -> Self {
        StateError::IOError {
            error: err.to_string(),
        }
    }
}

impl ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        match self {
            SqlValue::Integer(i) => i.to_sql(),
            SqlValue::Real(f) => f.to_sql(),
            SqlValue::Text(ref s) => s.to_sql(),
            SqlValue::Blob(ref b) => b.to_sql(),
            SqlValue::Boolean(b) => b.to_sql(),
            SqlValue::Null => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Null,
            )),
        }
    }
}

impl FromSql for SqlValue {
    fn column_result(value: ValueRef<'_>) -> Result<Self, FromSqlError> {
        match value {
            ValueRef::Integer(i) => Ok(SqlValue::Integer(i)),
            ValueRef::Real(f) => Ok(SqlValue::Real(f)),
            ValueRef::Text(t) => {
                let text_str = std::str::from_utf8(t).map_err(|_| FromSqlError::InvalidType)?;
                Ok(SqlValue::Text(text_str.to_string()))
            }
            ValueRef::Blob(b) => Ok(SqlValue::Blob(b.to_vec())),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

impl std::fmt::Display for SqliteAction {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<std::io::Error> for SqliteError {
    fn from(err: std::io::Error) -> Self {
        SqliteError::IOError {
            error: err.to_string(),
        }
    }
}

impl From<rusqlite::Error> for SqliteError {
    fn from(err: rusqlite::Error) -> Self {
        SqliteError::RusqliteError {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for SqliteError {
    fn from(err: tokio::sync::oneshot::error::RecvError) -> Self {
        SqliteError::NoCap {
            error: err.to_string(),
        }
    }
}

impl From<tokio::sync::mpsc::error::SendError<CapMessage>> for SqliteError {
    fn from(err: tokio::sync::mpsc::error::SendError<CapMessage>) -> Self {
        SqliteError::NoCap {
            error: err.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimerAction {
    Debug,
    SetTimer(u64),
}

//
// networking protocol types
//

/// Must be parsed from message pack vector.
/// all Get actions must be sent from local process. used for debugging
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetAction {
    /// Received from a router of ours when they have a new pending passthrough for us.
    /// We should respond (if we desire) by using them to initialize a routed connection
    /// with the NodeId given.
    ConnectionRequest(NodeId),
    /// can only receive from trusted source, for now just ourselves locally,
    /// in the future could get from remote provider
    KnsUpdate(KnsUpdate),
    KnsBatchUpdate(Vec<KnsUpdate>),
    /// get a list of peers we are connected to
    GetPeers,
    /// get the [`Identity`] struct for a single peer
    GetPeer(String),
    /// get the [`NodeId`] associated with a given namehash, if any
    GetName(String),
    /// get a user-readable diagnostics string containing networking inforamtion
    GetDiagnostics,
    /// sign the attached blob payload, sign with our node's networking key.
    /// **only accepted from our own node**
    /// **the source [`Address`] will always be prepended to the payload**
    Sign,
    /// given a message in blob payload, verify the message is signed by
    /// the given source. if the signer is not in our representation of
    /// the PKI, will not verify.
    /// **the `from` [`Address`] will always be prepended to the payload**
    Verify {
        from: Address,
        signature: Vec<u8>,
    },
}

/// For now, only sent in response to a ConnectionRequest.
/// Must be parsed from message pack vector
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetResponse {
    Accepted(NodeId),
    Rejected(NodeId),
    /// response to [`NetAction::GetPeers`]
    Peers(Vec<Identity>),
    /// response to [`NetAction::GetPeer`]
    Peer(Option<Identity>),
    /// response to [`NetAction::GetName`]
    Name(Option<String>),
    /// response to [`NetAction::GetDiagnostics`]. a user-readable string.
    Diagnostics(String),
    /// response to [`NetAction::Sign`]. contains the signature in blob
    Signed,
    /// response to [`NetAction::Verify`]. boolean indicates whether
    /// the signature was valid or not. note that if the signer node
    /// cannot be found in our representation of PKI, this will return false,
    /// because we cannot find the networking public key to verify with.
    Verified(bool),
}

#[derive(Clone, Debug, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct KnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ips: Vec<String>,
    pub ports: BTreeMap<String, u16>,
    pub routers: Vec<String>,
}

impl KnsUpdate {
    pub fn get_protocol_port(&self, protocol: &str) -> u16 {
        self.ports.get(protocol).cloned().unwrap_or(0)
    }
}
