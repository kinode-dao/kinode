use serde::{Deserialize, Serialize};

use super::bindings::component::uq_process::types::*;
use super::bindings::{Address, Payload, ProcessId, SendError};

#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct PackageId {
    pub package_name: String,
    pub publisher_node: String,
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
    pub fn to_string(&self) -> String {
        [self.package_name.as_str(), self.publisher_node.as_str()].join(":")
    }
    pub fn package(&self) -> &str {
        &self.package_name
    }
    pub fn publisher_node(&self) -> &str {
        &self.publisher_node
    }
}

#[allow(dead_code)]
impl ProcessId {
    /// generates a random u64 number if process_name is not declared
    pub fn new(process_name: &str, package_name: &str, publisher_node: &str) -> Self {
        ProcessId {
            process_name: process_name.into(),
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
    pub fn to_string(&self) -> String {
        [
            self.process_name.as_str(),
            self.package_name.as_str(),
            self.publisher_node.as_str(),
        ]
        .join(":")
    }
    pub fn process(&self) -> &str {
        &self.process_name
    }
    pub fn package(&self) -> &str {
        &self.package_name
    }
    pub fn publisher_node(&self) -> &str {
        &self.publisher_node
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

impl PartialEq for ProcessId {
    fn eq(&self, other: &Self) -> bool {
        self.process_name == other.process_name
            && self.package_name == other.package_name
            && self.publisher_node == other.publisher_node
    }
}

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

impl Address {
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
    pub fn to_string(&self) -> String {
        [self.node.as_str(), &self.process.to_string()].join("@")
    }
}

#[derive(Debug)]
pub enum AddressParseError {
    TooManyColons,
    MissingNodeId,
    MissingField,
}

pub fn send_and_await_response(
    target: &Address,
    inherit: bool,
    ipc: Option<Json>,
    metadata: Option<Json>,
    payload: Option<&Payload>,
    timeout: u64,
) -> Result<(Address, Message), SendError> {
    super::bindings::send_and_await_response(
        target,
        &Request {
            inherit,
            expects_response: Some(timeout),
            ipc,
            metadata,
        },
        payload,
    )
}

pub fn send_request(
    target: &Address,
    inherit: bool,
    ipc: Option<Json>,
    metadata: Option<Json>,
    context: Option<&Json>,
    payload: Option<&Payload>,
) {
    super::bindings::send_request(
        target,
        &Request {
            inherit,
            expects_response: None,
            ipc,
            metadata,
        },
        context,
        payload,
    )
}

pub fn get_state<T: serde::de::DeserializeOwned>() -> Option<T> {
    match super::bindings::get_state() {
        Some(bytes) => match bincode::deserialize::<T>(&bytes) {
            Ok(state) => Some(state),
            Err(_) => None,
        },
        None => None,
    }
}

pub fn set_state<T>(state: &T)
where
    T: serde::Serialize,
{
    super::bindings::set_state(&bincode::serialize(state).unwrap());
}

pub fn parse_message_ipc<T>(json_string: Option<String>) -> anyhow::Result<T>
where
    for<'a> T: serde::Deserialize<'a>,
{
    let parsed: T = serde_json::from_str(
        json_string
            .ok_or(anyhow::anyhow!("json payload empty"))?
            .as_str(),
    )?;
    Ok(parsed)
}

//  move these to better place!
#[derive(Serialize, Deserialize, Debug)]
pub enum FsAction {
    Write,
    Replace(u128),
    Append(Option<u128>),
    Read(u128),
    ReadChunk(ReadChunkRequest),
    Delete(u128),
    Length(u128),
    //  process state management
    GetState,
    SetState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReadChunkRequest {
    pub file_uuid: u128,
    pub start: u64,
    pub length: u64,
}
