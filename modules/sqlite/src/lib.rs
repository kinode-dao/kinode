cargo_component_bindings::generate!();

use core::ffi::{c_char, c_int, c_ulonglong, CStr};
use std::ffi::CString;

use serde::{Deserialize, Serialize};

use bindings::component::uq_process::types::*;
use bindings::{get_payload, Guest, print_to_terminal, receive, send_and_await_response, send_response};

mod kernel_types;
use kernel_types as kt;
mod process_lib;

struct Component;

const PREFIX: &str = "sqlite-";


#[repr(C)]
pub struct COptionStr {
    is_empty: c_int,        // 0 -> string is empty
    string: *const c_char,
}

#[repr(C)]
struct CBytes {
    data: *mut u8,
    len: usize,
}

#[repr(C)]
pub struct CPayload {
    is_empty: c_int,          // 0 -> payload is empty
    mime: *const COptionStr,
    bytes: *mut CBytes,
}

#[repr(C)]
pub struct CProcessId {
    result_number: c_int,  // 0 -> first (u64), !0 -> second (String)
    id: c_ulonglong,
    name: *const c_char,
}

#[repr(C)]
pub struct CIpcMetadata {
    ipc: *const COptionStr,
    metadata: *const COptionStr,
}

impl COptionStr {
    fn new(s: Option<String>) -> Self {
        let (is_empty, string) = match s {
            None => (0, CString::new("").expect("")),
            Some(s) => (1, CString::new(s).expect("")),
        };
        COptionStr {
            is_empty,
            string: string.as_ptr(),
        }
    }

    fn as_ptr(self) -> *const Self {
        Box::into_raw(Box::new(self))
    }
}

fn from_coptionstr_to_option_string(s: *const COptionStr) -> Option<String> {
    if unsafe { (*s).is_empty == 0 } {
        None
    } else {
        Some(from_cstr_to_string(unsafe { (*s).string }))
    }
}

impl CBytes {
    fn new(mut bytes: Vec<u8>) -> Self {
        CBytes {
            data: bytes.as_mut_ptr(),
            len: bytes.len(),
        }
    }

    fn new_empty() -> Self {
        CBytes::new(Vec::with_capacity(0))
    }

    fn as_mut_ptr(self) -> *mut Self {
        Box::into_raw(Box::new(self))
    }
}

impl From<Vec<u8>> for CBytes {
    fn from(bytes: Vec<u8>) -> Self {
        CBytes::new(bytes)
    }
}

impl From<CBytes> for Vec<u8> {
    fn from(bytes: CBytes) -> Self {
        let bytes = unsafe { Vec::from_raw_parts(bytes.data, bytes.len, bytes.len) };
        bytes
    }
}

fn from_cbytes_to_vec_u8(bytes: *mut CBytes) -> Vec<u8> {
    let bytes = unsafe { Vec::from_raw_parts((*bytes).data, (*bytes).len, (*bytes).len) };
    bytes
}

impl From<Option<Payload>> for CPayload {
    fn from(p: Option<Payload>) -> Self {
        let (is_empty, mime, bytes) = match p {
            None => (0, COptionStr::new(None).as_ptr(), CBytes::new_empty().as_mut_ptr()),
            Some(Payload { mime, bytes }) => (1, COptionStr::new(mime).as_ptr(), CBytes::new(bytes).as_mut_ptr()),
        };
        CPayload {
            is_empty,
            mime,
            bytes,
        }
    }
}

impl From<CPayload> for Option<Payload> {
    fn from(p: CPayload) -> Self {
        if p.is_empty == 0 {
            None
        } else {
            let mime = from_coptionstr_to_option_string(p.mime);
            let bytes = from_cbytes_to_vec_u8(p.bytes);
            Some(Payload {
                mime,
                bytes,
            })
        }
    }
}

fn from_cpayload_to_option_payload(p: *const CPayload) -> Option<Payload> {
    if unsafe { (*p).is_empty == 0 } {
        None
    } else {
        let mime = unsafe { from_coptionstr_to_option_string((*p).mime) };
        let bytes = unsafe { from_cbytes_to_vec_u8((*p).bytes) };
        Some(Payload {
            mime,
            bytes,
        })
    }
}

impl From<CProcessId> for ProcessId {
    fn from(pid: CProcessId) -> Self {
        if pid.result_number == 0 {
            ProcessId::Id(pid.id)
        } else {
            ProcessId::Name(from_cstr_to_string(pid.name))
        }
    }
}

fn from_cprocessid_to_processid(pid: *const CProcessId) -> ProcessId {
    if unsafe { (*pid).result_number } == 0 {
        ProcessId::Id(unsafe { (*pid).id })
    } else {
        ProcessId::Name(from_cstr_to_string(unsafe { (*pid).name }))
    }
}

fn from_cstr_to_string(s: *const c_char) -> String {
    let cstr = unsafe { CStr::from_ptr(s) };
    cstr.to_str().unwrap().into()
}

#[no_mangle]
pub extern "C" fn get_payload_wrapped() -> *mut CPayload {
    let mut payload = get_payload().into();
    std::ptr::addr_of_mut!(payload)
}

impl CIpcMetadata {
    fn new(ipc: *const COptionStr, metadata: *const COptionStr) -> Self {
        CIpcMetadata {
            ipc,
            metadata,
        }
    }

    fn as_ptr(self) -> *const Self {
        Box::into_raw(Box::new(self))
    }
}

#[no_mangle]
pub extern "C" fn send_and_await_response_wrapped(
    target_node: *const c_char,
    target_process: *const CProcessId,
    request_ipc: *const COptionStr,
    request_metadata: *const COptionStr,
    payload: *const CPayload,
    timeout: c_ulonglong,
) -> *const CIpcMetadata {
    let target_node = from_cstr_to_string(target_node);
    let target_process = from_cprocessid_to_processid(target_process);
    let payload = from_cpayload_to_option_payload(payload);
    let request_ipc = from_coptionstr_to_option_string(request_ipc);
    let request_metadata = from_coptionstr_to_option_string(request_metadata);
    let (
        _,
        Message::Response((Response { ipc, metadata }, _)),
    ) = send_and_await_response(
        &Address {
            node: target_node,
            process: target_process,
        },
        &Request {
            inherit: false,
            expects_response: Some(timeout),
            ipc: request_ipc,
            metadata: request_metadata,
        },
        match payload {
            None => None,
            Some(ref p) => Some(p),
        },
    ).unwrap() else {
        panic!("");
    };
    CIpcMetadata::new(COptionStr::new(ipc).as_ptr(), COptionStr::new(metadata).as_ptr()).as_ptr()
}

fn handle_message (
    our: &Address,
    db: &mut Option<rusqlite::Connection>,
) -> anyhow::Result<()> {
    let (source, message) = receive().unwrap();
    // let (source, message) = receive()?;

    if our.node != source.node {
        return Err(anyhow::anyhow!(
            "rejecting foreign Message from {:?}",
            source,
        ));
    }

    match message {
        Message::Response(_) => { unimplemented!() },
        Message::Request(Request { ipc, .. }) => {
            match process_lib::parse_message_ipc(ipc.clone())? {
                kt::SqliteMessage::New { identifier: sql_identifier } => {
                    let vfs_identifier = format!("{}{}", PREFIX, sql_identifier);
                    match db {
                        Some(_) => {
                            return Err(anyhow::anyhow!("cannot send New more than once"));
                        },
                        None => {
                            let flags = rusqlite::OpenFlags::default();
                            *db = Some(rusqlite::Connection::open_with_flags_and_vfs(
                                format!(
                                    "/{}.sql",
                                    sql_identifier,
                                ),
                                flags,
                                "demo",
                            )?);
                        },
                    }
                },
                kt::SqliteMessage::Write { ref key, .. } => {
                    // let Some(db) = db else {
                    //     return Err(anyhow::anyhow!("cannot send New more than once"));
                    // };

                    // let Payload { mime: _, ref bytes } = get_payload().ok_or(anyhow::anyhow!("couldnt get bytes for Write"))?;

                    // let write_txn = db.begin_write()?;
                    // {
                    //     let mut table = write_txn.open_table(TABLE)?;
                    //     table.insert(&key[..], &bytes[..])?;
                    // }
                    // write_txn.commit()?;

                    // send_response(
                    //     &Response {
                    //         ipc,
                    //         metadata: None,
                    //     },
                    //     None,
                    // );
                },
                kt::SqliteMessage::Read { ref key, .. } => {
                    // let Some(db) = db else {
                    //     return Err(anyhow::anyhow!("cannot send New more than once"));
                    // };

                    // let read_txn = db.begin_read()?;

                    // let table = read_txn.open_table(TABLE)?;

                    // match table.get(&key[..])? {
                    //     None => {
                    //         send_response(
                    //             &Response {
                    //                 ipc,
                    //                 metadata: None,
                    //             },
                    //             None,
                    //         );
                    //     },
                    //     Some(v) => {
                    //         send_response(
                    //             &Response {
                    //                 ipc,
                    //                 metadata: None,
                    //             },
                    //             Some(&Payload {
                    //                 mime: None,
                    //                 bytes: v.value().to_vec(),
                    //             }),
                    //         );
                    //     },
                    // };
                },
            }

            Ok(())
        },
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "sqlite: begin");

        let mut db: Option<rusqlite::Connection> = None;

        loop {
            match handle_message(&our, &mut db) {
                Ok(()) => {},
                Err(e) => {
                    //  TODO: should we send an error on failure?
                    print_to_terminal(0, format!(
                        "key_value_worker: error: {:?}",
                        e,
                    ).as_str());
                },
            };
        }
    }
}
