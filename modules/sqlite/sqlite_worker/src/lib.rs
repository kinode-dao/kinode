use core::ffi::{c_char, c_int, c_ulonglong, CStr};
use std::ffi::CString;

use rusqlite::{types::FromSql, types::FromSqlError, types::ToSql, types::ValueRef};
use std::collections::HashMap;

use uqbar_process_lib::{Address, ProcessId, Response, grant_messaging};
use uqbar_process_lib::uqbar::process::standard as wit;

use crate::sqlite_types::Deserializable;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

mod sqlite_types;
use sqlite_types as sq;

const PREFIX: &str = "sqlite-";

impl ToSql for sq::SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput> {
        match self {
            sq::SqlValue::Integer(i) => i.to_sql(),
            sq::SqlValue::Real(f) => f.to_sql(),
            sq::SqlValue::Text(ref s) => s.to_sql(),
            sq::SqlValue::Blob(ref b) => b.to_sql(),
            sq::SqlValue::Boolean(b) => b.to_sql(),
            sq::SqlValue::Null => Ok(rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Null)),
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
    fn new(s: Option<Vec<u8>>) -> Self {
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
    fn new(s: Option<Vec<u8>>) -> Self {
        let (is_empty, string) = match s {
            None => (0, CString::new("").unwrap()),
            Some(s) => (1, CString::new(s).unwrap()),
        };
        COptionStr {
            is_empty,
            string: string.as_ptr() as *mut c_char,
        }
    }
}

fn from_coptionstr_to_bytes(s: *const COptionStr) -> Vec<u8> {
    if unsafe { (*s).is_empty == 0 } {
        vec![]
    } else {
        from_cstr_to_string(unsafe { (*s).string }).as_bytes().to_vec()
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

impl From<Option<wit::Payload>> for CPrePayload {
    fn from(p: Option<wit::Payload>) -> Self {
        let (is_empty, mime, bytes) = match p {
            None => (0, COptionStr::new(None), CBytes::new_empty()),
            Some(wit::Payload { mime, bytes }) => {
                let mime = match mime {
                    Some(s) => Some(s.as_bytes().to_vec()),
                    None => None,
                };
                (1, COptionStr::new(mime), CBytes::new(bytes))
            }
        };
        CPrePayload {
            is_empty,
            mime,
            bytes,
        }
    }
}

impl From<CPayload> for Option<wit::Payload> {
    fn from(p: CPayload) -> Self {
        if p.is_empty == 0 {
            None
        } else {
            let mime = from_coptionstr_to_option_string(p.mime);
            let bytes = from_cbytes_to_vec_u8(p.bytes);
            Some(wit::Payload {
                mime,
                bytes,
            })
        }
    }
}

fn from_cpayload_to_option_payload(p: *const CPayload) -> Option<wit::Payload> {
    if unsafe { (*p).is_empty == 0 } {
        None
    } else {
        let mime = unsafe { from_coptionstr_to_option_string((*p).mime) };
        let bytes = unsafe { from_cbytes_to_vec_u8((*p).bytes) };
        Some(wit::Payload {
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
    let cstr = unsafe { CStr::from_ptr(s) };
    cstr.to_str().unwrap().into()
}

#[no_mangle]
pub extern "C" fn get_payload_wrapped(return_val: *mut CPayload) {
    // TODO: remove this logic; just here to avoid writing to invalid places
    // in memory due to an fs bug where chunk size may be bigger than requested
    let max_len = unsafe { (*(*return_val).bytes).len.clone() };

    let payload = wit::get_payload();
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
            },
        }
    }
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
    let target_node = from_cstr_to_string(target_node);
    let target_process = from_cprocessid_to_processid(target_process);
    let payload = from_cpayload_to_option_payload(payload);
    let request_ipc = from_coptionstr_to_bytes(request_ipc);
    let request_metadata = from_coptionstr_to_option_string(request_metadata);
    let (
        _,
        wit::Message::Response((wit::Response { ipc, metadata, .. }, _)),
    ) = wit::send_and_await_response(
        &wit::Address {
            node: target_node,
            process: target_process,
        },
        &wit::Request {
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
    let ipc = CPreOptionStr::new(Some(ipc));
    let metadata = CPreOptionStr::new(match metadata {
        None => None,
        Some(s) => Some(s.as_bytes().to_vec())
    });

    CIpcMetadata::copy_to_ptr(return_val, ipc, metadata);
}

fn json_to_sqlite(value: &serde_json::Value) -> Result<sq::SqlValue, sq::SqliteError> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(int_val) = n.as_i64() {
                Ok(sq::SqlValue::Integer(int_val))
            } else if let Some(float_val) = n.as_f64() {
                Ok(sq::SqlValue::Real(float_val))
            } else {
                Err(sq::SqliteError::InvalidParameters)
            }
        },
        serde_json::Value::String(s) => {
            match base64::decode(&s) {
                Ok(decoded_bytes) => {
                    // Convert to SQLite Blob if it's a valid base64 string
                    Ok(sq::SqlValue::Blob(decoded_bytes))
                },
                Err(_) => {
                    // If it's not base64, just use the string itself
                    Ok(sq::SqlValue::Text(s.clone()))
                }
            }
        },
        serde_json::Value::Bool(b) => {
            Ok(sq::SqlValue::Boolean(*b))
        },
        serde_json::Value::Null => {
            Ok(sq::SqlValue::Null)
        },
        _ => {
            Err(sq::SqliteError::InvalidParameters)
        }
    }
}

fn handle_message(
    our: &wit::Address,
    conn: &mut Option<rusqlite::Connection>,
    txs: &mut HashMap<u64, Vec<(String, Vec<sq::SqlValue>)>>,
) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(sq::SqliteError::RejectForeign.into());
    }

    match message {
        wit::Message::Response(_) => { unimplemented!() },
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
                sq::SqliteMessage::New { db } => {
                    let vfs_drive = format!("{}{}", PREFIX, db);

                    match conn {
                        Some(_) => {
                            return Err(sq::SqliteError::DbAlreadyExists.into());
                        },
                        None => {
                            let flags = rusqlite::OpenFlags::default();
                            *conn = Some(rusqlite::Connection::open_with_flags_and_vfs(
                                format!(
                                    "{}:{}:/{}.sql",
                                    our.node,
                                    vfs_drive,
                                    db,
                                ),
                                flags,
                                "uqbar",
                            )?);
                        },
                    }
                },
                sq::SqliteMessage::Write { ref statement, tx_id, .. } => {
                    let Some(ref conn) = conn else {
                        return Err(sq::SqliteError::DbDoesNotExist.into());
                    };

                    let parameters: Vec<sq::SqlValue> = match wit::get_payload() {
                        None => vec![],
                        Some(wit::Payload { mime: _, ref bytes }) => {
                            let json_params = serde_json::from_slice::<serde_json::Value>(bytes)?;
                            match json_params {
                                serde_json::Value::Array(vec) => {
                                    vec.iter().map(|value| json_to_sqlite(value)).collect::<Result<Vec<_>, _>>()?
                                },
                                _ => {
                                    return Err(sq::SqliteError::InvalidParameters.into());
                                }
                            }
                        },
                    };

                    match tx_id {
                        Some(tx_id) => {
                            txs.entry(tx_id)
                                .or_insert_with(Vec::new)
                                .push((statement.clone(), parameters));
                        },
                        None => {
                            let mut stmt = conn.prepare(statement)?;
                            stmt.execute(rusqlite::params_from_iter(parameters.iter()))?;
                        },
                    };

                    Response::new()
                        .ipc_bytes(ipc)
                        .send()?;
                },
                sq::SqliteMessage::Commit { ref tx_id, .. } => {
                    let Some(queries) = txs.remove(tx_id) else {
                        return Err(sq::SqliteError::NoTx.into());
                    };

                    let Some(ref mut conn) = conn else {
                        return Err(sq::SqliteError::DbDoesNotExist.into());
                    };

                    let tx = conn.transaction()?;
                    for (query, params) in queries {
                        tx.execute(&query, rusqlite::params_from_iter(params.iter()))?;
                    }

                    tx.commit()?;

                    Response::new()
                        .ipc_bytes(ipc)
                        .send()?;
                },
                sq::SqliteMessage::Read { ref query, .. } => {
                    let Some(ref db_handle) = conn else {
                        return Err(sq::SqliteError::DbDoesNotExist.into());
                    };

                    let parameters: Vec<sq::SqlValue> = match wit::get_payload() {
                        None => vec![],
                        Some(wit::Payload { mime: _, ref bytes }) => {
                            let json_params = serde_json::from_slice::<serde_json::Value>(bytes)?;
                            match json_params {
                                serde_json::Value::Array(vec) => {
                                    vec.iter().map(|value| json_to_sqlite(value)).collect::<Result<Vec<_>, _>>()?
                                },
                                _ => {
                                    return Err(sq::SqliteError::InvalidParameters.into());
                                }
                            }
                        },
                    };

                    let mut statement = db_handle.prepare(query)?;
                    let column_names: Vec<String> = statement
                        .column_names()
                        .iter()
                        .map(|c| c.to_string())
                        .collect();

                    let results: Vec<HashMap<String, serde_json::Value>> = statement
                    .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
                        let mut map = HashMap::new();
                        for (i, column_name) in column_names.iter().enumerate() {
                            let value: sq::SqlValue = row.get(i)?;
                            let value_json = match value {
                                sq::SqlValue::Integer(int) => serde_json::Value::Number(int.into()),
                                sq::SqlValue::Real(real) => serde_json::Value::Number(serde_json::Number::from_f64(real).unwrap()),
                                sq::SqlValue::Text(text) => serde_json::Value::String(text),
                                sq::SqlValue::Blob(blob) => serde_json::Value::String(base64::encode(blob)), // or another representation if you prefer
                                _ => serde_json::Value::Null,
                            };
                            map.insert(column_name.clone(), value_json);
                        }
                        Ok(map)
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                    
                    let results = serde_json::json!(results).to_string();
                    let results_bytes = results.as_bytes().to_vec();

                    Response::new()
                        .ipc_bytes(ipc)
                        .payload(wit::Payload {
                            mime: None,
                            bytes: results_bytes,
                        })
                        .send()?;
                },
            }

            Ok(())
        },
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(1, "sqlite_worker: begin");
        let our = Address::from_str(&our).unwrap();

        let mut conn: Option<rusqlite::Connection> = None;
        let mut txs: HashMap<u64, Vec<(String, Vec<sq::SqlValue>)>> = HashMap::new();

        grant_messaging(
            &our,
            &Vec::from([ProcessId::from_str("vfs:sys:uqbar").unwrap()])
        );

        loop {
            match handle_message(&our, &mut conn, &mut txs) {
                Ok(()) => {},
                Err(e) => {
                    //  TODO: should we send an error on failure?
                    wit::print_to_terminal(0, format!(
                        "sqlite_worker: error: {:?}",
                        e,
                    ).as_str());
                    if let Some(e) = e.downcast_ref::<sq::SqliteError>() {
                        Response::new()
                            .ipc_bytes(serde_json::to_vec(&e).unwrap())
                            .send()
                            .unwrap();
                    }
                },
            };
        }
    }
}
