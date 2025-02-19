use crate::hyperware::process::contacts;
use hyperware_process_lib::{
    await_message, call_init, eth, get_blob, get_typed_state, homepage, http, hypermap, set_state,
    Address, Capability, LazyLoadBlob, Message, NodeId, Response,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

wit_bindgen::generate!({
    path: "target/wit",
    world: "contacts-sys-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize, process_macros::SerdeJsonInto],
});

const ICON: &str = include_str!("icon");

#[cfg(not(feature = "simulation-mode"))]
const CHAIN_ID: u64 = hypermap::HYPERMAP_CHAIN_ID;
#[cfg(feature = "simulation-mode")]
const CHAIN_ID: u64 = 31337; // local

const CHAIN_TIMEOUT: u64 = 60; // 60s

#[cfg(not(feature = "simulation-mode"))]
const HYPERMAP_ADDRESS: &'static str = hypermap::HYPERMAP_ADDRESS; // base
#[cfg(feature = "simulation-mode")]
const HYPERMAP_ADDRESS: &str = "0xEce71a05B36CA55B895427cD9a440eEF7Cf3669D";

#[derive(Debug, Serialize, Deserialize)]
struct Contact(HashMap<String, serde_json::Value>);

#[derive(Debug, Serialize, Deserialize)]
struct Contacts(HashMap<NodeId, Contact>);

#[derive(Debug, Serialize, Deserialize)]
struct ContactsStateV1 {
    our: Address,
    contacts: Contacts,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "version")]
enum VersionedState {
    /// State fully stored in memory, persisted using serde_json.
    /// Future state version will use SQLite.
    V1(ContactsStateV1),
}

impl VersionedState {
    fn new(our: Address) -> Self {
        get_typed_state(|bytes| serde_json::from_slice(bytes)).unwrap_or(Self::V1(
            ContactsStateV1 {
                our,
                contacts: Contacts(HashMap::new()),
            },
        ))
    }

    fn save(&self) {
        set_state(&serde_json::to_vec(&self).expect("Failed to serialize contacts state!"));
    }

    fn contacts(&self) -> &Contacts {
        match self {
            VersionedState::V1(state) => &state.contacts,
        }
    }

    fn get_contact(&self, node: NodeId) -> Option<&Contact> {
        match self {
            VersionedState::V1(state) => state.contacts.0.get(&node),
        }
    }

    fn add_contact(&mut self, node: NodeId) {
        match self {
            VersionedState::V1(state) => {
                state.contacts.0.insert(node, Contact(HashMap::new()));
            }
        }
        self.save();
    }

    fn remove_contact(&mut self, node: NodeId) {
        match self {
            VersionedState::V1(state) => {
                state.contacts.0.remove(&node);
            }
        }
        self.save();
    }

    fn add_field(&mut self, node: NodeId, field: String, value: serde_json::Value) {
        match self {
            VersionedState::V1(state) => {
                state
                    .contacts
                    .0
                    .entry(node)
                    .or_insert_with(|| Contact(HashMap::new()))
                    .0
                    .insert(field, value);
            }
        }
        self.save();
    }

    fn remove_field(&mut self, node: NodeId, field: String) {
        match self {
            VersionedState::V1(state) => {
                if let Some(contact) = state.contacts.0.get_mut(&node) {
                    contact.0.remove(&field);
                }
            }
        }
        self.save();
    }

    fn ws_update(&self, http_server: &mut http::server::HttpServer) {
        http_server.ws_push_all_channels(
            "/",
            http::server::WsMessageType::Text,
            LazyLoadBlob::new(
                Some("application/json"),
                serde_json::to_vec(self.contacts()).unwrap(),
            ),
        );
    }

    fn our(&self) -> &Address {
        match self {
            VersionedState::V1(state) => &state.our,
        }
    }
}

call_init!(initialize);
fn initialize(our: Address) {
    homepage::add_to_homepage("Contacts", Some(ICON), Some("/"), None);

    let mut state: VersionedState = get_typed_state(|bytes| serde_json::from_slice(bytes))
        .unwrap_or_else(|| VersionedState::new(our));

    let hypermap = hypermap::Hypermap::new(
        eth::Provider::new(CHAIN_ID, CHAIN_TIMEOUT),
        eth::Address::from_str(HYPERMAP_ADDRESS).unwrap(),
    );

    let mut http_server = http::server::HttpServer::new(5);

    // serve the frontend on a secure subdomain
    http_server
        .serve_ui(
            "ui",
            vec!["/"],
            http::server::HttpBindingConfig::default().secure_subdomain(true),
        )
        .unwrap();
    http_server.secure_bind_http_path("/ask").unwrap();
    http_server.secure_bind_ws_path("/").unwrap();

    main_loop(&mut state, &hypermap, &mut http_server);
}

fn main_loop(
    state: &mut VersionedState,
    hypermap: &hypermap::Hypermap,
    http_server: &mut http::server::HttpServer,
) {
    loop {
        match await_message() {
            Err(_send_error) => {
                // ignore send errors, local-only process
                continue;
            }
            Ok(Message::Request {
                source,
                body,
                capabilities,
                ..
            }) => {
                // ignore messages from other nodes -- technically superfluous check
                // since manifest does not acquire networking capability
                if source.node() != state.our().node {
                    continue;
                }
                handle_request(&source, &body, capabilities, state, hypermap, http_server);
            }
            _ => continue, // ignore responses
        }
    }
}

fn handle_request(
    source: &Address,
    body: &[u8],
    capabilities: Vec<Capability>,
    state: &mut VersionedState,
    hypermap: &hypermap::Hypermap,
    http_server: &mut http::server::HttpServer,
) {
    // source node is ALWAYS ourselves since networking is disabled
    if source.process == "http-server:distro:sys" {
        // receive HTTP requests and websocket connection messages from our server
        let server_request = http_server.parse_request(body).unwrap();

        http_server.handle_request(
            server_request,
            |req| handle_http_request(state, hypermap, &req),
            |_channel_id, _message_type, _blob| {
                // we don't expect websocket messages
            },
        );
    } else {
        // if request is not from frontend, check that it has the required capabilities
        let (response, blob) = handle_contacts_request(state, hypermap, body, Some(capabilities));
        let mut response = Response::new().body(response);
        if let Some(blob) = blob {
            response = response.blob(blob);
        }
        response.send().unwrap();
    }
    state.ws_update(http_server);
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    state: &mut VersionedState,
    hypermap: &hypermap::Hypermap,
    http_request: &http::server::IncomingHttpRequest,
) -> (http::server::HttpResponse, Option<LazyLoadBlob>) {
    match http_request.method().unwrap().as_str() {
        "GET" => (
            http::server::HttpResponse::new(http::StatusCode::OK)
                .header("Content-Type", "application/json"),
            Some(LazyLoadBlob::new(
                Some("application/json"),
                serde_json::to_vec(state.contacts()).unwrap(),
            )),
        ),
        "POST" => {
            let blob = get_blob().unwrap();
            let (response, blob) = handle_contacts_request(state, hypermap, blob.bytes(), None);
            if let contacts::Response::Err(e) = response {
                return (
                    http::server::HttpResponse::new(http::StatusCode::BAD_REQUEST)
                        .header("Content-Type", "application/json"),
                    Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&e).unwrap(),
                    )),
                );
            }
            (
                http::server::HttpResponse::new(http::StatusCode::OK)
                    .header("Content-Type", "application/json"),
                match blob {
                    Some(blob) => Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&blob.bytes).unwrap(),
                    )),
                    None => None,
                },
            )
        }
        // Any other method will be rejected.
        _ => (
            http::server::HttpResponse::new(http::StatusCode::METHOD_NOT_ALLOWED),
            None,
        ),
    }
}

fn handle_contacts_request(
    state: &mut VersionedState,
    hypermap: &hypermap::Hypermap,
    request_bytes: &[u8],
    capabilities: Option<Vec<Capability>>,
) -> (contacts::Response, Option<LazyLoadBlob>) {
    let Ok(request) = serde_json::from_slice::<contacts::Request>(request_bytes) else {
        return (
            contacts::Response::Err("Malformed request".to_string()),
            None,
        );
    };
    // if request is not from frontend, check capabilities:
    // each request requires one of read-name-only, read, add, or remove
    if let Some(capabilities) = capabilities {
        let required_capability = Capability::new(
            state.our(),
            serde_json::to_string(&match request {
                contacts::Request::GetNames => contacts::Capability::ReadNameOnly,
                contacts::Request::GetAllContacts | contacts::Request::GetContact(_) => {
                    contacts::Capability::Read
                }
                contacts::Request::AddContact(_) | contacts::Request::AddField(_) => {
                    contacts::Capability::Add
                }
                contacts::Request::RemoveContact(_) | contacts::Request::RemoveField(_) => {
                    contacts::Capability::Remove
                }
            })
            .unwrap(),
        );
        if !capabilities.contains(&required_capability) {
            return (
                contacts::Response::Err("Missing capability".to_string()),
                None,
            );
        }
    }

    match request {
        contacts::Request::GetNames => (
            contacts::Response::GetNames(
                state
                    .contacts()
                    .0
                    .keys()
                    .map(|node| node.to_string())
                    .collect(),
            ),
            None,
        ),
        contacts::Request::GetAllContacts => (
            contacts::Response::GetAllContacts,
            Some(LazyLoadBlob::new(
                Some("application/json"),
                serde_json::to_vec(state.contacts()).unwrap(),
            )),
        ),
        contacts::Request::GetContact(node) => (
            contacts::Response::GetContact,
            Some(LazyLoadBlob::new(
                Some("application/json"),
                serde_json::to_vec(&state.get_contact(node)).unwrap(),
            )),
        ),
        contacts::Request::AddContact(node) => {
            if let Some((response, blob)) = invalid_node(hypermap, &node) {
                return (response, blob);
            }
            state.add_contact(node);
            (contacts::Response::AddContact, None)
        }
        contacts::Request::AddField((node, field, value)) => {
            if let Some((response, blob)) = invalid_node(hypermap, &node) {
                return (response, blob);
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&value) else {
                return (contacts::Response::Err("Malformed value".to_string()), None);
            };
            state.add_field(node, field, value);
            (contacts::Response::AddField, None)
        }
        contacts::Request::RemoveContact(node) => {
            state.remove_contact(node);
            (contacts::Response::RemoveContact, None)
        }
        contacts::Request::RemoveField((node, field)) => {
            state.remove_field(node, field);
            (contacts::Response::RemoveField, None)
        }
    }
}

fn invalid_node(
    hypermap: &hypermap::Hypermap,
    node: &str,
) -> Option<(contacts::Response, Option<LazyLoadBlob>)> {
    if hypermap
        .get(&node)
        .map(|(tba, _, _)| tba != eth::Address::ZERO)
        .unwrap_or(false)
    {
        None
    } else {
        Some((
            contacts::Response::Err("Node name invalid or does not exist".to_string()),
            None,
        ))
    }
}
