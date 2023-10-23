cargo_component_bindings::generate!();

use core::ffi::{c_char, c_int, c_ulonglong, CStr};
use std::ffi::CString;

use crate::sqlite_types::Deserializable;

use rusqlite::{Connection, types::FromSql, types::FromSqlError, types::ToSql, types::Value, types::ValueRef};
// use serde::{Deserialize, Serialize};

use bindings::component::uq_process::types::*;
use bindings::{get_payload, Guest, print_to_terminal, receive, send_and_await_response, send_response};

mod kernel_types;
use kernel_types as kt;
mod process_lib;
mod sqlite_types;
use sqlite_types as sq;

struct Component;

const PREFIX: &str = "sqlite-";

impl ToSql for sq::SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        match self {
            sq::SqlValue::Integer(i) => i.to_sql(),
            sq::SqlValue::Real(f) => f.to_sql(),
            sq::SqlValue::Text(ref s) => s.to_sql(),
            sq::SqlValue::Blob(ref b) => b.to_sql(),
        }
    }
}

impl FromSql for sq::SqlValue {
    fn column_result(value: ValueRef<'_>) -> Result<Self, FromSqlError> {
        match value {
            ValueRef::Integer(i) => Ok(sq::SqlValue::Integer(i)),
            ValueRef::Real(f) => Ok(sq::SqlValue::Real(f)),
            ValueRef::Text(t) => {
                let text_str = std::str::from_utf8(t).map_err(|_| FromSqlError::InvalidType)?;
                Ok(sq::SqlValue::Text(text_str.to_string()))
            },
            ValueRef::Blob(b) => Ok(sq::SqlValue::Blob(b.to_vec())),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}

#[repr(C)]
pub struct CPreOptionStr {
    is_empty: c_int,        // 0 -> string is empty
    string: CString,
}

#[repr(C)]
pub struct COptionStr {
    is_empty: c_int,        // 0 -> string is empty
    string: *mut c_char,
}

#[repr(C)]
struct CBytes {
    data: *mut u8,
    len: usize,
}

#[repr(C)]
pub struct CPayload {
    is_empty: c_int,          // 0 -> payload is empty
    mime: *mut COptionStr,
    bytes: *mut CBytes,
}

#[repr(C)]
pub struct CPrePayload {
    is_empty: c_int,          // 0 -> payload is empty
    mime: COptionStr,
    bytes: CBytes,
}

#[repr(C)]
pub struct CProcessId {
    process_name: *const c_char,
    package_name: *const c_char,
    publisher_node: *const c_char,
}

#[repr(C)]
pub struct CIpcMetadata {
    ipc: *mut COptionStr,
    metadata: *mut COptionStr,
}

impl CPreOptionStr {
    fn new(s: Option<String>) -> Self {
        let (is_empty, string) = match s {
            None => (0, CString::new("").unwrap()),
            Some(s) => (1, CString::new(s).unwrap()),
        };
        CPreOptionStr {
            is_empty,
            string,
        }
    }
}

impl COptionStr {
    fn new(s: Option<String>) -> Self {
        let (is_empty, string) = match s {
            None => (0, CString::new("").unwrap()),
            Some(s) => (1, CString::new(s).unwrap()),
        };
        COptionStr {
            is_empty,
            string: string.as_ptr() as *mut c_char,
        }
    }

    fn as_ptr(self) -> *const Self {
        Box::into_raw(Box::new(self))
    }

    fn as_mut_ptr(self) -> *mut Self {
        Box::into_raw(Box::new(self))
    }
}

fn from_coptionstr_to_option_string(s: *const COptionStr) -> Option<String> {
    //print_to_terminal(0, "fctos");
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
    // let bytes = unsafe { Vec::from_raw_parts((*bytes).data, (*bytes).len, (*bytes).len) };
    let bytes = unsafe { std::slice::from_raw_parts((*bytes).data, (*bytes).len) };
    let bytes = bytes.to_vec();
    bytes
}

impl From<Option<Payload>> for CPrePayload {
    fn from(p: Option<Payload>) -> Self {
        let (is_empty, mut mime, bytes) = match p {
            None => (0, COptionStr::new(None), CBytes::new_empty()),
            Some(Payload { mime, bytes }) => (1, COptionStr::new(mime), CBytes::new(bytes)),
        };
        CPrePayload {
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

fn from_cprocessid_to_processid(pid: *const CProcessId) -> ProcessId {
    ProcessId {
        process_name: from_cstr_to_string(unsafe { (*pid).process_name }),
        package_name: from_cstr_to_string(unsafe { (*pid).package_name }),
        publisher_node: from_cstr_to_string(unsafe { (*pid).publisher_node }),
    }
}

fn from_cstr_to_string(s: *const c_char) -> String {
    // print_to_terminal(0, "fcts 0");
    let cstr = unsafe { CStr::from_ptr(s) };
    // print_to_terminal(0, "fcts 1");
    // let a = cstr.to_str();
    // print_to_terminal(0, "fcts 1a");
    // let b = a.unwrap();
    // print_to_terminal(0, &format!("fcts 1b {}", b));
    // let c = b.to_string();
    // print_to_terminal(0, "fcts 1c");
    // c
    cstr.to_str().unwrap().into()
}

#[no_mangle]
pub extern "C" fn get_payload_wrapped(return_val: *mut CPayload) {
    print_to_terminal(0, "gpw 0");
    // TODO: remove this logic; just here to avoid writing to invalid places
    // in memory due to an fs bug where chunk size may be bigger than requested
    let max_len = unsafe { (*(*return_val).bytes).len.clone() };

    let payload = get_payload();
    let mime_len = {
        match payload {
            None => None,
            Some(ref payload) => {
                match payload.mime {
                    None => None,
                    Some(ref mime) => {
                        Some(mime.len())
                    },
                }
            }
        }
    };
    // print_to_terminal(0, &format!("{:?}", payload));
    // print_to_terminal(0, &format!("{:?}", opayload));
    // print_to_terminal(0, &format!("{:?}", unsafe { *(payload.bytes.data) }));
    // print_to_terminal(0, &format!("gpw: copying {} bytes", unsafe { payload.bytes.len }));
    unsafe {
        match payload {
            None => {},
            Some(payload) => {
                (*return_val).is_empty = 1;
                match payload.mime {
                    None => {},
                    Some(mime) => {
                        (*(*return_val).mime).is_empty = 1;
                        let Some(mime_len) = mime_len else { panic!("") };
                        let mime = CString::new(mime).unwrap();
                        std::ptr::copy_nonoverlapping(
                            mime.as_ptr(),
                            (*(*return_val).mime).string,
                            mime_len + 1,
                        );
                    },
                }
                (*(*return_val).bytes).len = std::cmp::min(max_len, payload.bytes.len());
                std::ptr::copy_nonoverlapping(
                    payload.bytes.as_ptr(),
                    (*(*return_val).bytes).data,
                    std::cmp::min(max_len, payload.bytes.len()),
                );
                // print_to_terminal(0, &format!("{:?} {}", unsafe { (*(*(*return_val).bytes).data) }, std::cmp::min(max_len, payload.bytes.len())));
            },
        }
    }
    print_to_terminal(0, "gpw: done copying");
}

impl CIpcMetadata {
    fn copy_to_ptr(ptr: *mut CIpcMetadata, ipc: CPreOptionStr, metadata: CPreOptionStr) {
        unsafe {
            (*(*ptr).ipc).is_empty = ipc.is_empty;
            if ipc.is_empty == 1 {
                std::ptr::copy_nonoverlapping(
                    ipc.string.as_ptr(),
                    (*(*ptr).ipc).string,
                    ipc.string.as_bytes_with_nul().len(),
                );
            }
            (*(*ptr).metadata).is_empty = metadata.is_empty;
            if metadata.is_empty == 1 {
                std::ptr::copy_nonoverlapping(
                    metadata.string.as_ptr(),
                    (*(*ptr).metadata).string,
                    metadata.string.as_bytes_with_nul().len(),
                );
            }
        }
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
    return_val: *mut CIpcMetadata,
) {
    print_to_terminal(0, "saarw: start");
    let target_node = from_cstr_to_string(target_node);
    print_to_terminal(0, "saarw: a");
    let target_process = from_cprocessid_to_processid(target_process);
    print_to_terminal(0, "saarw: b");
    let payload = from_cpayload_to_option_payload(payload);
    print_to_terminal(0, "saarw: c");
    let request_ipc = from_coptionstr_to_option_string(request_ipc);
    print_to_terminal(0, "saarw: d");
    let request_metadata = from_coptionstr_to_option_string(request_metadata);
    print_to_terminal(0, "saarw: e");
    let (
        _,
        Message::Response((Response { ipc, metadata, .. }, _)),
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
    let ipc = CPreOptionStr::new(ipc);
    let metadata = CPreOptionStr::new(metadata);

    print_to_terminal(0, "saarw: copying");
    CIpcMetadata::copy_to_ptr(return_val, ipc, metadata);
    print_to_terminal(0, "saarw: done copying");
}

#[no_mangle]
pub extern "C" fn print_to_terminal_wrapped(verbosity: c_int, content: c_int) {
    print_to_terminal(verbosity as u8, &format!("sqlite(C): {}", content));
}

fn handle_message (
    our: &Address,
    db_handle: &mut Option<rusqlite::Connection>,
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
                sq::SqliteMessage::New { db } => {
                    let vfs_address = Address {
                        node: our.node.clone(),
                        process: kt::ProcessId::new("vfs", "sys", "uqbar").en_wit(),
                    };
                    let vfs_drive = format!("{}{}", PREFIX, db);

                    let _ = process_lib::send_and_await_response(
                        &vfs_address,
                        false,
                        Some(serde_json::to_string(&kt::VfsRequest {
                            drive: vfs_drive.clone(),
                            action: kt::VfsAction::New,
                        }).unwrap()),
                        None,
                        None,
                        15,
                    ).unwrap();

                    match db_handle {
                        Some(_) => {
                            return Err(anyhow::anyhow!("cannot send New more than once"));
                        },
                        None => {
                            let flags = rusqlite::OpenFlags::default();
                            *db_handle = Some(rusqlite::Connection::open_with_flags_and_vfs(
                                format!(
                                    "{}:{}:/{}.sql",
                                    our.node,
                                    vfs_drive,
                                    db,
                                ),
                                flags,
                                "demo",
                            )?);
                        },
                    }
                    print_to_terminal(0, "sqlite: New done");
                    send_response(
                        &Response {
                            inherit: false,
                            ipc,
                            metadata: None,
                        },
                        None,
                    );
                },
                sq::SqliteMessage::Write { ref statement, .. } => {
                    let Some(db_handle) = db_handle else {
                        return Err(anyhow::anyhow!("need New before Write"));
                    };

                    match get_payload() {
                        None => {
                            let parameters: Vec<&dyn rusqlite::ToSql> = vec![];
                            db_handle.execute(
                                statement,
                                &parameters[..],
                            )?;
                        },
                        Some(Payload { mime: _, ref bytes }) => {
                            let parameters = Vec::<sq::SqlValue>::from_serialized(&bytes)?;
                            let parameters: Vec<&dyn rusqlite::ToSql> = parameters
                                .iter()
                                .map(|param| param as &dyn rusqlite::ToSql)
                                .collect();

                            db_handle.execute(
                                statement,
                                &parameters[..],
                            )?;
                        },
                    }

                    send_response(
                        &Response {
                            inherit: false,
                            ipc,
                            metadata: None,
                        },
                        None,
                    );
                },
                sq::SqliteMessage::Read { ref query, .. } => {
                    let Some(db_handle) = db_handle else {
                        return Err(anyhow::anyhow!("need New before Write"));
                    };

                    let mut statement = db_handle.prepare(query)?;
                    let column_names: Vec<String> = statement
                        .column_names()
                        .iter()
                        .map(|c| c.to_string())
                        .collect();
                    let number_columns = column_names.len();
                    let results: Vec<Vec<sq::SqlValue>> = statement
                        .query_map([], |row| {
                            (0..number_columns)
                                .map(|i| row.get(i))
                                .collect()
                            })?
                        .map(|item| item.unwrap())  //  TODO
                        .collect();

                    let results = rmp_serde::to_vec(&results).unwrap();

                    send_response(
                        &Response {
                            inherit: false,
                            ipc,
                            metadata: None,
                        },
                        Some(&Payload {
                            mime: None,
                            bytes: results,
                        }),
                    );
                },
            }

            Ok(())
        },
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "sqlite: begin");

        let mut db_handle: Option<rusqlite::Connection> = None;

        loop {
            match handle_message(&our, &mut db_handle) {
                Ok(()) => {},
                Err(e) => {
                    //  TODO: should we send an error on failure?
                    print_to_terminal(0, format!(
                        "sqlite: error: {:?}",
                        e,
                    ).as_str());
                },
            };
        }
    }
}
