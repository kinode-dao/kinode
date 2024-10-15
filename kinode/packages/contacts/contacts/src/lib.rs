use crate::kinode::process::contacts::{ContactsRequest, ContactsResponse};
use kinode_process_lib::{
    await_message, call_init, get_blob, get_typed_state, homepage, http, println, set_state,
    Address, LazyLoadBlob, Message, NodeId, Response,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const ICON: &str = include_str!("icon");

#[derive(Debug, Serialize, Deserialize)]
struct Contact(HashMap<String, serde_json::Value>);

#[derive(Debug, Serialize, Deserialize)]
struct Contacts(HashMap<NodeId, Contact>);

#[derive(Debug, Serialize, Deserialize)]
struct ContactsState {
    our: Address,
    contacts: Contacts,
}

impl ContactsState {
    fn new(our: Address) -> Self {
        get_typed_state(|bytes| serde_json::from_slice(bytes)).unwrap_or(Self {
            our,
            contacts: Contacts(HashMap::new()),
        })
    }

    fn save(&self) {
        set_state(&serde_json::to_vec(&self).expect("Failed to serialize contacts state!"));
    }

    fn contacts(&self) -> &Contacts {
        &self.contacts
    }

    fn get_contact(&self, node: NodeId) -> Option<&Contact> {
        self.contacts.0.get(&node)
    }

    fn add_contact(&mut self, node: NodeId) {
        self.contacts.0.insert(node, Contact(HashMap::new()));
    }

    fn remove_contact(&mut self, node: NodeId) {
        self.contacts.0.remove(&node);
    }

    fn add_field(&mut self, node: NodeId, field: String, value: serde_json::Value) {
        self.contacts
            .0
            .entry(node)
            .or_insert_with(|| Contact(HashMap::new()))
            .0
            .insert(field, value);
    }

    fn remove_field(&mut self, node: NodeId, field: String) {
        if let Some(contact) = self.contacts.0.get_mut(&node) {
            contact.0.remove(&field);
        }
    }
}

wit_bindgen::generate!({
    path: "target/wit",
    world: "contacts-sys-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize],
});

call_init!(initialize);
fn initialize(our: Address) {
    homepage::add_to_homepage("Contacts", Some(ICON), Some("/"), None);

    let mut state: ContactsState = ContactsState::new(our);

    let mut http_server = http::server::HttpServer::new(5);

    // serve the frontend on a secure subdomain
    http_server
        .serve_ui(
            &state.our,
            "ui",
            vec!["/"],
            http::server::HttpBindingConfig::default().secure_subdomain(true),
        )
        .unwrap();
    http_server.secure_bind_http_path("/ask").unwrap();
    http_server.secure_bind_ws_path("/").unwrap();

    main_loop(&mut state, &mut http_server);
}

fn main_loop(state: &mut ContactsState, http_server: &mut http::server::HttpServer) {
    loop {
        match await_message() {
            Err(_send_error) => {
                // ignore send errors, local-only process
                continue;
            }
            Ok(Message::Request {
                source,
                body,
                expects_response,
                ..
            }) => {
                if source.node() != state.our.node {
                    continue; // ignore messages from other nodes
                }
                let response = handle_request(&source, &body, state, http_server);
                // state.ws_update(http_server);
                if expects_response.is_some() {
                    Response::new()
                        .body(serde_json::to_vec(&response).unwrap())
                        .send()
                        .unwrap();
                }
            }
            _ => continue, // ignore responses
        }
    }
}

fn handle_request(
    source: &Address,
    body: &[u8],
    state: &mut ContactsState,
    http_server: &mut http::server::HttpServer,
) -> Option<ContactsResponse> {
    // source node is ALWAYS ourselves since networking is disabled
    if source.process == "http_server:distro:sys" {
        // receive HTTP requests and websocket connection messages from our server
        let server_request = http_server.parse_request(body).unwrap();

        http_server.handle_request(
            server_request,
            |req| handle_http_request(state, &req),
            |_channel_id, _message_type, _blob| {
                // we don't expect websocket messages
            },
        );
        None
    } else {
        // let settings_request = serde_json::from_slice::<SettingsRequest>(body)
        //     .map_err(|_| SettingsError::MalformedRequest)?;
        // handle_settings_request(state, settings_request)
        None
    }
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    state: &mut ContactsState,
    http_request: &http::server::IncomingHttpRequest,
) -> (http::server::HttpResponse, Option<LazyLoadBlob>) {
    match http_request.method().unwrap().as_str() {
        "GET" => {
            // state.fetch().unwrap();
            (
                http::server::HttpResponse::new(http::StatusCode::OK)
                    .header("Content-Type", "application/json"),
                Some(LazyLoadBlob::new(
                    Some("application/json"),
                    serde_json::to_vec(&state).unwrap(),
                )),
            )
        }
        "POST" => {
            let blob = get_blob().unwrap();
            let request = serde_json::from_slice::<ContactsRequest>(&blob.bytes).unwrap();
            let response = handle_contacts_request(state, request);
            let response: Option<String> = Some("ok".to_string());
            (
                http::server::HttpResponse::new(http::StatusCode::OK)
                    .header("Content-Type", "application/json"),
                match response {
                    Some(data) => Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&data).unwrap(),
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
    state: &mut ContactsState,
    request: ContactsRequest,
) -> ContactsResponse {
    match request {
        ContactsRequest::GetNames => ContactsResponse::GetNames(
            state
                .contacts()
                .0
                .keys()
                .map(|node| node.to_string())
                .collect(),
        ),
        ContactsRequest::GetAllContacts => ContactsResponse::GetAllContacts,
        ContactsRequest::GetContact(node) => ContactsResponse::GetContact,
        ContactsRequest::AddContact(node) => ContactsResponse::AddContact,
        ContactsRequest::AddField((node, field, value)) => ContactsResponse::AddField,
        ContactsRequest::RemoveContact(node) => ContactsResponse::RemoveContact,
        ContactsRequest::RemoveField((node, field)) => ContactsResponse::RemoveField,
    }
}
