use serde::{Deserialize, Serialize};

use super::bindings::component::uq_process::types::*;
use super::bindings::{Address, Payload, SendError};

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

pub enum ProcessIdParseError {
    TooManyColons,
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

pub fn get_state(our: String) -> Option<Payload> {
    match super::bindings::get_state() {
        Some(bytes) => Some(Payload { mime: None, bytes }),
        None => None,
    }
}

pub fn set_state(our: String, bytes: Vec<u8>) {
    super::bindings::set_state(&bytes);
}

pub fn await_set_state<T>(our: String, state: &T)
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
