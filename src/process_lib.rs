use serde::{Deserialize, Serialize};

use super::bindings::component::uq_process::types::*;
use super::bindings::{Address, get_payload, Payload, SendError, send_request};

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
        Some(bytes) => Some(Payload {
            mime: None,
            bytes,
        }),
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
