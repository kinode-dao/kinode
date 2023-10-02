cargo_component_bindings::generate!();

use bindings::component::uq_process::types::*;
use bindings::{
    get_payload, print_to_terminal, receive, save_capabilities, send_and_await_response,
    send_request, send_requests, send_response, Guest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, to_vec};
use std::collections::HashMap;
extern crate base64;

mod process_lib;

// process_lib::set_state our, bytes
// process_lib::await_set_state our, any serializable type
// process_lib::get_state -> Option<Payload> gets the entire state

struct Component;

type Contact = HashMap<String, String>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OrgChat {
    id: i64,
    invite_link: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Member {
    username: String,
    is_admin: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Org {
    id: u64,
    owner: String,
    name: String,
    description: String,
    members: HashMap<String, Member>,
    chats: HashMap<String, OrgChat>,
    created: u64,
    updated: u64,
}

type Orgs = HashMap<u64, Org>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TelegramChat {
    id: i64,
    title: String,
    chat_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TelegramBot {
    id: u64,
    is_bot: bool,
    first_name: String,
    username: String,
    can_join_groups: bool,
    can_read_all_group_messages: bool,
    supports_inline_queries: bool,
    token: String,
    chats: HashMap<i64, TelegramChat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct OrgsState {
    pub our_contact_info: Contact,
    pub address_book: HashMap<String, Contact>,
    pub requester_updates: HashMap<String, bool>,
    pub orgs: Orgs,
    pub telegram_bots: HashMap<u64, TelegramBot>,
}

fn generate_http_binding(
    add: Address,
    path: &str,
    authenticated: bool,
) -> (Address, Request, Option<Context>, Option<Payload>) {
    (
        add,
        Request {
            inherit: false,
            expects_response: None,
            ipc: Some(
                json!({
                    "action": "bind-app",
                    "path": path,
                    "app": "orgs",
                    "authenticated": authenticated
                })
                .to_string(),
            ),
            metadata: None,
        },
        None,
        None,
    )
}

fn get_http_request_info(
    message_json: serde_json::Value,
) -> (
    String,            // method
    String,            // path
    String,            // raw_path
    serde_json::Value, // headers
    serde_json::Value, // url_params
    serde_json::Value, // query_params
) {
    let method = message_json["method"].as_str().unwrap_or("").to_string();
    let path = message_json["path"].as_str().unwrap_or("").to_string();
    let raw_path = message_json["raw_path"].as_str().unwrap_or("").to_string();
    let headers = message_json["headers"].clone();
    let url_params = message_json["url_params"].clone();
    let query_params = message_json["query_params"].clone();

    (method, path, raw_path, headers, url_params, query_params)
}

fn send_http_response(status: u16, headers: HashMap<String, String>, payload_bytes: Vec<u8>) {
    send_response(
        &Response {
            ipc: Some(
                json!({
                    "status": status,
                    "headers": headers,
                })
                .to_string(),
            ),
            metadata: None,
        },
        Some(&Payload {
            mime: Some("application/octet-stream".to_string()),
            bytes: payload_bytes,
        }),
    )
}

fn get_response_info(
    response: Result<(Address, Message), SendError>,
) -> (Option<String>, Option<Payload>, Option<String>) {
    match response {
        Ok((_source, message)) => {
            if let Message::Response((response, context)) = message {
                let ipc = match response.ipc {
                    Some(ipc) => Some(ipc.to_string()),
                    None => None,
                };
                (ipc, get_payload(), context)
            } else {
                (None, None, None)
            }
        }
        Err(_) => (None, None, None),
    }
}

fn send_http_client_request(
    our_name: String,
    url: String,
    method: &str,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    context: Option<String>,
) {
    send_request(
        &Address {
            node: our_name,
            process: ProcessId::Name("http_client".to_string()),
        },
        &Request {
            inherit: false,
            expects_response: Some(5), // TODO evaluate timeout
            ipc: Some(
                json!({
                    "method": method,
                    "uri": url,
                    "headers": headers,
                })
                .to_string(),
            ),
            metadata: None,
        },
        context.as_ref(),
        Some(&Payload {
            mime: Some("application/octet-stream".to_string()),
            bytes: body,
        }),
    )
}

fn call_telegram_api(
    our_name: String,
    token: String,
    path: String,
    method: &str,
    body: serde_json::Value,
) {
    send_http_client_request(
        our_name.clone(),
        format!("https://api.telegram.org/bot{}/{}", token, path),
        method,
        {
            let mut headers = HashMap::new();
            headers.insert("Content-Type".to_string(), "application/json".to_string());
            headers
        },
        body.to_string().as_bytes().to_vec(),
        None,
    )
}

fn modify_telegram_membership(
    org: &Org,
    our_name: String,
    telegram_bots: &HashMap<u64, TelegramBot>,
    address_book: &HashMap<String, Contact>,
    username: String,
    action: &str,
) {
    if let Some(chat) = org.chats.get("telegram") {
        // find the right bot for this chat
        for b in telegram_bots.values() {
            if b.chats.contains_key(&chat.id) {
                if let Some(contact) = address_book.get(&username) {
                    if let Some(telegram_id) = contact.get("telegram_id") {
                        // print_to_terminal(0, format!("orgs: {} USER {}", action.to_string(), telegram_id).as_str());
                        if let Ok(telegram_id) = telegram_id.parse::<u64>() {
                            call_telegram_api(
                                our_name,
                                b.token.clone(),
                                action.to_string(),
                                "POST",
                                json!({
                                    "chat_id": chat.id,
                                    "user_id": telegram_id,
                                }),
                            );
                        }
                    }
                }
                break;
            }
        }
    }
}

fn handle_telegram_update(
    our_name: String,
    bot_id: u64,
    json: serde_json::Value,
    orgs: &mut Orgs,
    telegram_bots: &mut HashMap<u64, TelegramBot>,
    address_book: &mut HashMap<String, Contact>,
) -> Option<u64> {
    let update_result = json["result"].clone();
    let mut update_id: Option<u64> = None;
    let Some(bot_data) = telegram_bots.get_mut(&bot_id) else {
        return update_id;
    };

    if let Some(result_array) = update_result.as_array() {
        for result in result_array {
            if let Some(result_object) = result.as_object() {
                update_id = match result_object.get("update_id") {
                    Some(update_id) => match update_id.as_u64() {
                        Some(update_id) => Some(update_id),
                        None => None,
                    },
                    None => None,
                };

                if let Some(message) = result_object.get("message") {
                    // handle everything here
                    let chat = &message["chat"];
                    let Some(chat_id) = chat["id"].as_i64() else {
                        return None;
                    };
                    let existing_chat = bot_data.chats.get(&chat_id);

                    if existing_chat.is_none() {
                        let telegram_chat = TelegramChat {
                            id: chat_id,
                            title: chat["title"].as_str().unwrap_or_default().to_string(),
                            chat_type: chat["type"].as_str().unwrap_or_default().to_string(),
                        };
                        bot_data.chats.insert(chat_id, telegram_chat.clone());

                        call_telegram_api(
                            our_name.clone(),
                            bot_data.token.clone(),
                            "sendMessage".to_string(),
                            "POST",
                            json!({
                                "chat_id": chat_id,
                                "text": format!("I have registered this chat in the API manager! ({})", chat["title"].as_str().unwrap())
                            }),
                        );

                        send_request(
                            &Address {
                                node: our_name.clone(),
                                process: ProcessId::Name("encryptor".to_string()),
                            },
                            &Request {
                                inherit: false,
                                expects_response: None,
                                ipc: Some(
                                    json!({
                                        "EncryptAndForwardAction": {
                                            "channel_id": "orgs",
                                            "forward_to": {
                                                "node": our_name.clone(),
                                                "process": {
                                                    "Name": "http_server"
                                                },
                                            },
                                            "json": Some(json!({ // this is the JSON to forward
                                                "WebSocketPush": {
                                                    "target": {
                                                        "node": our_name.clone(),
                                                        "id": "orgs", // If the message passed in an ID then we could send to just that ID
                                                    }
                                                }
                                            })),
                                        }

                                    })
                                    .to_string(),
                                ),
                                metadata: None,
                            },
                            None,
                            Some(&Payload {
                                mime: Some("application/json".to_string()),
                                bytes: json!({
                                    "kind": "telegram_chat_added",
                                    "data": {
                                        "bot_id": bot_id,
                                        "chat": telegram_chat,
                                    }
                                })
                                .to_string()
                                .as_bytes()
                                .to_vec(),
                            }),
                        );

                        let response = send_and_await_response(
                            &Address {
                                node: our_name.clone(),
                                process: ProcessId::Name("http_client".to_string()),
                            },
                            &Request {
                                inherit: false,
                                expects_response: Some(5), // TODO evaluate timeout
                                ipc: Some(json!({
                                    "method": "GET",
                                    "uri": format!("https://api.telegram.org/bot{}/getChatAdministrators", bot_data.token),
                                    "headers": {
                                        "Content-Type": "application/json",
                                    },
                                }).to_string()),
                                metadata: None,
                            },
                            Some(&Payload {
                                mime: Some("application/json".to_string()),
                                bytes: json!({
                                    "chat_id": chat_id,
                                }).to_string().as_bytes().to_vec(),
                            }),
                        );

                        match get_response_info(response) {
                            (Some(ipc), Some(payload), _) => {
                                let json =
                                    serde_json::from_slice::<serde_json::Value>(&payload.bytes)
                                        .unwrap();
                                if let Some(admins) = json["result"].as_array() {
                                    // Iterate over the admins and check if the bot is an admin
                                    for admin in admins {
                                        let user_id =
                                            admin["user"]["id"].as_u64().unwrap_or_default();
                                        let can_manage_chat =
                                            admin["can_manage_chat"].as_bool().unwrap_or_default();

                                        if user_id == bot_id && !can_manage_chat {
                                            call_telegram_api(
                                                our_name.clone(),
                                                bot_data.token.clone(),
                                                "sendMessage".to_string(),
                                                "POST",
                                                json!({
                                                    "chat_id": chat_id,
                                                    "text": "Please go to chat info and make me an admin so that I can manage this chat. You should allow me to \"Invite Users via Link\"."
                                                }),
                                            );
                                        }
                                    }
                                }
                            }
                            _ => (),
                        }
                        print_to_terminal(0, "1.5");
                    }

                    if let Some(chat_join_request) = message["chat_join_request"].as_object() {
                        let Some(chat_id) = chat["id"].as_i64() else {
                            return None;
                        };
                        let from = &chat_join_request["from"];

                        // do a for loop over orgs and check if the chat_id is in any of the orgs
                        for org in orgs.values() {
                            if let Some(telegram_chat) = org.chats.get("telegram") {
                                if telegram_chat.id == chat_id {
                                    // this is the org we want
                                    let mut is_member = false;
                                    for (member, _) in &org.members {
                                        if let Some(mut contact) = address_book.get_mut(member) {
                                            if contact
                                                .get("telegram_username")
                                                .unwrap_or(&"".to_string())
                                                == from["username"].as_str().unwrap_or("none")
                                            {
                                                contact.insert(
                                                    "telegram_id".to_string(),
                                                    from["id"].to_string(),
                                                );
                                                call_telegram_api(
                                                    our_name.clone(),
                                                    bot_data.token.clone(),
                                                    "unbanChatMember".to_string(),
                                                    "POST",
                                                    json!({
                                                        "chat_id": chat_id,
                                                        "user_id": from["id"],
                                                    }),
                                                );
                                                call_telegram_api(
                                                    our_name.clone(),
                                                    bot_data.token.clone(),
                                                    "approveChatJoinRequest".to_string(),
                                                    "POST",
                                                    json!({
                                                        "chat_id": chat_id,
                                                        "user_id": from["id"],
                                                    }),
                                                );

                                                is_member = true;
                                                break;
                                            }
                                        }
                                    }

                                    if !is_member {
                                        print_to_terminal(0, "7");
                                        call_telegram_api(
                                            our_name.clone(),
                                            bot_data.token.clone(),
                                            "declineChatJoinRequest".to_string(),
                                            "POST",
                                            json!({
                                                "chat_id": chat_id,
                                                "user_id": from["id"],
                                            }),
                                        );
                                    }
                                }
                            }
                        }
                    } else if let Some(new_chat_members) = message["new_chat_members"].as_array() {
                        let Some(chat_id) = chat["id"].as_i64() else {
                            return None;
                        };
                        for ncm in new_chat_members.iter() {
                            for org in orgs.values() {
                                if let Some(telegram_chat) = org.chats.get("telegram") {
                                    if telegram_chat.id == chat_id {
                                        for (member, _) in &org.members {
                                            if let Some(mut contact) = address_book.get_mut(member)
                                            {
                                                if let Some(telegram_username) =
                                                    contact.get("telegram_username")
                                                {
                                                    if ncm["username"].as_str().unwrap_or("none")
                                                        == telegram_username
                                                    {
                                                        contact.insert(
                                                            "telegram_id".to_string(),
                                                            ncm["id"].to_string(),
                                                        );
                                                        return update_id;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            call_telegram_api(
                                our_name.clone(),
                                bot_data.token.clone(),
                                "banChatMember".to_string(),
                                "POST",
                                json!({
                                    "chat_id": chat_id,
                                    "user_id": ncm["id"],
                                }),
                            );
                            call_telegram_api(
                                our_name.clone(),
                                bot_data.token.clone(),
                                "unbanChatMember".to_string(),
                                "POST",
                                json!({
                                    "chat_id": chat_id,
                                    "user_id": ncm["id"],
                                }),
                            );
                        }
                    }
                }
            }
        }
    }

    update_id
}

fn self_is_admin(orgs: &Orgs, our_name: String, org_id: u64) -> bool {
    if let Some(org) = orgs.get(&org_id) {
        if let Some(member) = org.members.get(&our_name) {
            return member.is_admin;
        }
    }

    false
}

fn sum_char_codes(s: &str) -> u64 {
    s.chars().map(|c| c as u64).sum()
}

fn serve_html(our: Address, default_headers: HashMap<String, String>) {
    let response = send_and_await_response(
        &Address {
            node: our.node.clone(),
            process: ProcessId::Name("vfs".to_string()),
        },
        &Request {
            inherit: false,
            expects_response: Some(5), // TODO evaluate timeout
            ipc: Some(
                json!({
                    "GetEntry": {
                        "identifier": "orgs_static",
                        "full_path": "/index.html"
                    }
                })
                .to_string(),
            ),
            metadata: None,
        },
        None,
    );

    if let Some(payload) = get_payload() {
        send_http_response(200, default_headers.clone(), payload.bytes);
    } else {
        send_http_response(
            404,
            default_headers.clone(),
            "Not Found".to_string().as_bytes().to_vec(),
        );
    }
}

fn serve_static(raw_path: &str, our: Address, default_headers: HashMap<String, String>) {
    if let Some(file_path) = raw_path.strip_prefix("/orgs/static") {
        let mut headers = HashMap::new();
        let content_type = match file_path.split(".").last() {
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("png") => "image/png",
            Some("jpg") => "image/jpeg",
            Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("svg") => "image/svg+xml",
            _ => "text/plain",
        };
        headers.insert("Content-Type".to_string(), content_type.to_string());

        let response = send_and_await_response(
            &Address {
                node: our.node.clone(),
                process: ProcessId::Name("vfs".to_string()),
            },
            &Request {
                inherit: false,
                expects_response: Some(5), // TODO evaluate timeout
                ipc: Some(
                    json!({
                        "GetEntry": {
                            "identifier": "orgs_static",
                            "full_path": file_path // everything after "/orgs/static"
                        }
                    })
                    .to_string(),
                ),
                metadata: None,
            },
            None,
        );

        if let Some(payload) = get_payload() {
            send_http_response(200, headers, payload.bytes);
        } else {
            send_http_response(
                404,
                default_headers.clone(),
                "Not Found".to_string().as_bytes().to_vec(),
            );
        }
    } else {
        send_http_response(
            404,
            default_headers.clone(),
            "Not Found".to_string().as_bytes().to_vec(),
        );
    }
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "RPC: start");

        let mut state: OrgsState = match process_lib::get_state(our.node.clone()) {
            Some(payload) => match serde_json::from_slice::<OrgsState>(&payload.bytes) {
                Ok(state) => state,
                Err(_) => OrgsState {
                    our_contact_info: HashMap::new(),
                    address_book: HashMap::new(),
                    requester_updates: HashMap::new(),
                    orgs: HashMap::new(),
                    telegram_bots: HashMap::new(),
                },
            },
            None => OrgsState {
                our_contact_info: HashMap::new(),
                address_book: HashMap::new(),
                requester_updates: HashMap::new(),
                orgs: HashMap::new(),
                telegram_bots: HashMap::new(),
            },
        };

        // call set_state(our.node.clone(), bytes) whenever anything changes

        let bindings_address = Address {
            node: our.node.clone(),
            process: ProcessId::Name("http_bindings".to_string()),
        };

        // <address, request, option<context>, option<payload>>
        let http_endpoint_binding_requests: [(Address, Request, Option<Context>, Option<Payload>);
            7] = [
            generate_http_binding(bindings_address.clone(), "/orgs", false),
            generate_http_binding(bindings_address.clone(), "/orgs/static/*", false),
            generate_http_binding(bindings_address.clone(), "/orgs/my-info", false),
            generate_http_binding(bindings_address.clone(), "/orgs/list", false),
            generate_http_binding(bindings_address.clone(), "/orgs/:org_id/members", false),
            generate_http_binding(bindings_address.clone(), "/orgs/:org_id/chats", false),
            generate_http_binding(bindings_address.clone(), "/orgs/:platform/bots", false),
        ];
        send_requests(&http_endpoint_binding_requests);

        loop {
            let Ok((source, message)) = receive() else {
                print_to_terminal(0, "orgs: got network error");
                // TODO: handle network error. These will almost always be orgs updates or address_book updates
                continue;
            };
            // TODO: handle the Message::Response case. This will be for telegram bot messages sent to http_client
            match message {
                Message::Request(request) => {
                    if let Some(json) = request.ipc {
                        print_to_terminal(1, format!("orgs: JSON {}", json).as_str());
                        let message_json: serde_json::Value = match serde_json::from_str(&json) {
                            Ok(v) => v,
                            Err(_) => {
                                print_to_terminal(0, "orgs: failed to parse ipc JSON, skipping");
                                continue;
                            }
                        };

                        if message_json["action"] == "transfer_capability" {
                            print_to_terminal(1, "orgs: transfer_capability");
                            if let Some(payload) = get_payload() {
                                let signature = payload.bytes;
                                let node = message_json["info"]["issuer"]["node"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                let process = message_json["info"]["issuer"]["process"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                let params = message_json["info"]["params"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();

                                if node == "" || process == "" || params == "" {
                                    print_to_terminal(
                                        1,
                                        "orgs: transfer_capability: missing node, process, or params",
                                    );
                                    continue;
                                }

                                save_capabilities(&[SignedCapability {
                                    issuer: Address {
                                        node,
                                        process: ProcessId::Name(process),
                                    },
                                    params,
                                    signature,
                                }]);
                            }
                        } else if message_json["action"] == "get_contact_info" {
                            print_to_terminal(1, "orgs: get_contact_info");
                            send_response(
                                &Response {
                                    ipc: Some(
                                        json!({
                                            "action": "get_contact_info",
                                        })
                                        .to_string(),
                                    ),
                                    metadata: None,
                                },
                                Some(&Payload {
                                    mime: Some("application/json".to_string()),
                                    bytes: json!(&state.our_contact_info).to_string().as_bytes().to_vec(),
                                }),
                            );
                            continue;
                        } else if message_json["action"] == "update_contact_info" {
                            if let Some(payload) = get_payload() {
                                if let Ok(contact_info) =
                                    serde_json::from_slice::<Contact>(&payload.bytes)
                                {
                                    state.address_book.insert(source.node.clone(), contact_info.clone());
                                    process_lib::set_state(our.node.clone(), to_vec(&state).unwrap());
                                    send_response(
                                        &Response {
                                            ipc: Some(
                                                json!({
                                                    "action": "update_contact_info",
                                                })
                                                .to_string(),
                                            ),
                                            metadata: None,
                                        },
                                        None,
                                    );
                                };
                            }
                            continue;
                        } else if message_json["action"] == "update_orgs" {
                            if let Some(payload) = get_payload() {
                                if let Ok(org) = serde_json::from_slice::<Org>(&payload.bytes) {
                                    state.orgs.insert(org.id, org);
                                    send_response(
                                        &Response {
                                            ipc: Some(
                                                json!({
                                                    "action": "update_orgs",
                                                })
                                                .to_string(),
                                            ),
                                            metadata: None,
                                        },
                                        None,
                                    );
                                };
                            }
                            continue;
                        } else if source.node == our.node
                            && source.process == ProcessId::Name("http_bindings".to_string())
                        {
                            // Handle http request
                            let mut default_headers = HashMap::new();
                            default_headers
                                .insert("Content-Type".to_string(), "text/html".to_string());

                            let (method, path, raw_path, headers, url_params, query_params) =
                                get_http_request_info(message_json.clone());

                            match method.as_str() {
                                "GET" => match path.as_str() {
                                    "/orgs" => serve_html(our.clone(), default_headers.clone()),
                                    "/orgs/static/*" => serve_static(
                                        &raw_path,
                                        our.clone(),
                                        default_headers.clone(),
                                    ),
                                    "/orgs/my-info" => {
                                        send_http_response(
                                            200,
                                            default_headers.clone(),
                                            json!(&state.our_contact_info)
                                                .to_string()
                                                .as_bytes()
                                                .to_vec(),
                                        );
                                    }
                                    "/orgs/list" => {
                                        send_http_response(
                                            200,
                                            {
                                                let mut headers = HashMap::new();
                                                headers.insert(
                                                    "Content-Type".to_string(),
                                                    "application/json".to_string(),
                                                );
                                                headers
                                            },
                                            json!(&state.orgs).to_string().as_bytes().to_vec(),
                                        );
                                    }
                                    "/orgs/:platform/bots" => {
                                        if url_params["platform"] == "telegram" {
                                            send_http_response(
                                                200,
                                                {
                                                    let mut headers = HashMap::new();
                                                    headers.insert(
                                                        "Content-Type".to_string(),
                                                        "application/json".to_string(),
                                                    );
                                                    headers
                                                },
                                                json!(&state.telegram_bots)
                                                    .to_string()
                                                    .as_bytes()
                                                    .to_vec(),
                                            );
                                        } else {
                                            send_http_response(
                                                404,
                                                default_headers.clone(),
                                                "Not Found".to_string().as_bytes().to_vec(),
                                            );
                                        }
                                    }
                                    _ => send_http_response(
                                        404,
                                        default_headers.clone(),
                                        "Not Found".to_string().as_bytes().to_vec(),
                                    ),
                                },
                                "POST" => {
                                    print_to_terminal(0, format!("POST: {}", path).as_str());
                                    let Some(payload) = get_payload() else {
                                        print_to_terminal(
                                            0,
                                            "orgs: no bytes in payload, skipping...",
                                        );
                                        send_http_response(
                                            400,
                                            default_headers.clone(),
                                            "No payload".to_string().as_bytes().to_vec(),
                                        );
                                        continue;
                                    };

                                    match path.as_str() {
                                        "/orgs" => {
                                            let Ok(org) = serde_json::from_slice::<serde_json::Value>(
                                                &payload.bytes,
                                            ) else {
                                                print_to_terminal(0, "orgs: JSON is not valid");
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid JSON".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            };

                                            if let Some(name) = org["name"].as_str() {
                                                let org_id = sum_char_codes(name);

                                                let mut org = Org {
                                                    id: sum_char_codes(name),
                                                    owner: our.node.clone(),
                                                    name: name.to_string(),
                                                    description: org["description"]
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string(),
                                                    members: HashMap::new(),
                                                    chats: HashMap::new(),
                                                    created: 0,
                                                    updated: 0,
                                                };
                                                org.members.insert(
                                                    our.node.clone(),
                                                    Member {
                                                        username: our.node.clone(),
                                                        is_admin: true,
                                                    },
                                                );

                                                state.orgs.insert(org.id.clone(), org.clone());
                                                send_http_response(
                                                    201,
                                                    default_headers.clone(),
                                                    json!(org).to_string().as_bytes().to_vec(),
                                                );
                                            } else {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Org Name"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        }
                                        "/orgs/my-info" => {
                                            let Ok(my_info) =
                                                serde_json::from_slice::<HashMap<String, String>>(
                                                    &payload.bytes,
                                                )
                                            else {
                                                print_to_terminal(0, "orgs: JSON is not valid");
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid JSON".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            };

                                            for (key, value) in my_info {
                                                state.our_contact_info.insert(key, value);
                                            }

                                            process_lib::set_state(our.node.clone(), to_vec(&state).unwrap());

                                            send_http_response(
                                                201,
                                                default_headers.clone(),
                                                "Created".to_string().as_bytes().to_vec(),
                                            );
                                        }
                                        "/orgs/:org_id/members" => {
                                            if !self_is_admin(
                                                &state.orgs,
                                                our.node.clone(),
                                                url_params["org_id"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .parse::<u64>()
                                                    .unwrap_or(0),
                                            ) {
                                                send_http_response(
                                                    403,
                                                    default_headers.clone(),
                                                    "Forbidden".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            }
                                            let Ok(json) =
                                                serde_json::from_slice::<serde_json::Value>(
                                                    &payload.bytes,
                                                )
                                            else {
                                                print_to_terminal(0, "orgs: Username is not valid");
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Username"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                                continue;
                                            };

                                            let Some(username_str) = json["member"].as_str() else {
                                                print_to_terminal(0, "orgs: Username is not valid");
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Username"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                                continue;
                                            };
                                            let username = username_str.to_string();
                                            let is_admin =
                                                json["is_admin"].as_bool().unwrap_or(false);

                                            let org_id = match url_params["org_id"]
                                                .as_str()
                                                .unwrap_or("0")
                                                .parse::<u64>()
                                            {
                                                Ok(value) => value,
                                                Err(e) => {
                                                    print_to_terminal(
                                                        1,
                                                        format!(
                                                            "orgs: failed to parse org_id: {}",
                                                            e
                                                        )
                                                        .as_str(),
                                                    );
                                                    send_http_response(
                                                        400,
                                                        default_headers.clone(),
                                                        "Invalid Org ID"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                    continue;
                                                }
                                            };

                                            if let Some(org) = state.orgs.get_mut(&org_id) {
                                                org.members.insert(
                                                    username.clone(),
                                                    Member {
                                                        username: username.clone(),
                                                        is_admin,
                                                    },
                                                );
                                                // Get contact info for the user
                                                send_request(
                                                    &Address {
                                                        node: username.clone(),
                                                        process: ProcessId::Name(
                                                            "orgs".to_string(),
                                                        ),
                                                    },
                                                    &Request {
                                                        inherit: false,
                                                        expects_response: Some(5), // TODO evaluate timeout
                                                        ipc: Some(
                                                            json!({
                                                                "action": "get_contact_info",
                                                            })
                                                            .to_string(),
                                                        ),
                                                        metadata: None,
                                                    },
                                                    None,
                                                    None,
                                                );
                                                // Send the org to the user
                                                send_request(
                                                    &Address {
                                                        node: username.clone(),
                                                        process: ProcessId::Name(
                                                            "orgs".to_string(),
                                                        ),
                                                    },
                                                    &Request {
                                                        inherit: false,
                                                        expects_response: Some(15), // TODO evaluate timeout
                                                        ipc: Some(
                                                            json!({
                                                                "action": "update_orgs",
                                                            })
                                                            .to_string(),
                                                        ),
                                                        metadata: None,
                                                    },
                                                    None,
                                                    Some(&Payload {
                                                        mime: Some("application/json".to_string()),
                                                        bytes: json!(&org)
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    }),
                                                );
                                                send_http_response(
                                                    201,
                                                    default_headers.clone(),
                                                    "Created".to_string().as_bytes().to_vec(),
                                                );
                                            } else {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Org ID"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        }
                                        "/orgs/:org_id/chats" => {
                                            if !self_is_admin(
                                                &state.orgs,
                                                our.node.clone(),
                                                url_params["org_id"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .parse::<u64>()
                                                    .unwrap_or(0),
                                            ) {
                                                send_http_response(
                                                    403,
                                                    default_headers.clone(),
                                                    "Forbidden".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            }
                                            let Ok(body) =
                                                serde_json::from_slice::<serde_json::Value>(
                                                    &payload.bytes,
                                                )
                                            else {
                                                print_to_terminal(0, "orgs: JSON is not valid");
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid JSON".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            };
                                            let org_id = match url_params["org_id"]
                                                .as_str()
                                                .unwrap_or("0")
                                                .parse::<u64>()
                                            {
                                                Ok(value) => value,
                                                Err(e) => {
                                                    print_to_terminal(
                                                        1,
                                                        format!(
                                                            "orgs: failed to parse org_id: {}",
                                                            e
                                                        )
                                                        .as_str(),
                                                    );
                                                    send_http_response(
                                                        400,
                                                        default_headers.clone(),
                                                        "Invalid Org ID"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                    continue;
                                                }
                                            };

                                            let chat_id = body["id"].as_i64().unwrap_or_default();
                                            let platform =
                                                body["platform"].as_str().unwrap_or_default();

                                            let mut bot: Option<TelegramBot> = None;
                                            for b in state.telegram_bots.values() {
                                                if b.chats.contains_key(&chat_id) {
                                                    bot = Some(b.clone());
                                                    break;
                                                }
                                            }

                                            if let Some(bot) = bot {
                                                let response = send_and_await_response(
                                                    &Address {
                                                        node: our.node.clone(),
                                                        process: ProcessId::Name("http_client".to_string()),
                                                    },
                                                    &Request {
                                                        inherit: false,
                                                        expects_response: Some(5), // TODO evaluate timeout
                                                        ipc: Some(json!({
                                                            "method": "GET",
                                                            "uri": format!("https://api.telegram.org/bot{}/getChat", bot.token.clone()),
                                                            "headers": {
                                                                "Content-Type": "application/json",
                                                            },
                                                        }).to_string()),
                                                        metadata: None,
                                                    },
                                                    Some(&Payload {
                                                        mime: Some("application/json".to_string()),
                                                        bytes: json!({
                                                            "chat_id": chat_id,
                                                        }).to_string().as_bytes().to_vec(),
                                                    }),
                                                );
                                                print_to_terminal(0, "2");

                                                let Some(response_payload) = get_payload() else {
                                                    print_to_terminal(
                                                        0,
                                                        "orgs: no payload in response",
                                                    );
                                                    send_http_response(
                                                        500,
                                                        default_headers.clone(),
                                                        "Unable to get chat invite link"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                    continue;
                                                };

                                                let json =
                                                    serde_json::from_slice::<serde_json::Value>(
                                                        &response_payload.bytes,
                                                    );
                                                print_to_terminal(0, "3");

                                                if let Ok(result_json) = json {
                                                    let invite_link = result_json["result"]
                                                        ["invite_link"]
                                                        .as_str()
                                                        .unwrap_or_default()
                                                        .to_string();
                                                    // print invite_link
                                                    print_to_terminal(
                                                        1,
                                                        format!(
                                                            "orgs: invite link {}",
                                                            invite_link
                                                        )
                                                        .as_str(),
                                                    );
                                                    // print org_id
                                                    print_to_terminal(
                                                        1,
                                                        format!("orgs: org_id {}", org_id).as_str(),
                                                    );
                                                    if let Some(org) = state.orgs.get_mut(&org_id) {
                                                        org.chats.insert(
                                                            platform.to_string(),
                                                            OrgChat {
                                                                id: chat_id,
                                                                invite_link: invite_link,
                                                            },
                                                        );
                                                        print_to_terminal(0, "4");

                                                        for (member, _) in &org.members {
                                                            if let Some(contact) =
                                                                state.address_book.get(member)
                                                            {
                                                                if let Some(telegram_username) =
                                                                    contact.get("telegram_username")
                                                                {
                                                                    call_telegram_api(
                                                                        our.node.clone(),
                                                                        bot.token.clone(),
                                                                        "unbanChatMember"
                                                                            .to_string(),
                                                                        "POST",
                                                                        json!({
                                                                            "chat_id": chat_id,
                                                                            "user_id": telegram_username,
                                                                        }),
                                                                    );
                                                                }
                                                            }
                                                        }
                                                        print_to_terminal(0, "5");

                                                        send_http_response(
                                                            201,
                                                            default_headers.clone(),
                                                            json!(org)
                                                                .to_string()
                                                                .as_bytes()
                                                                .to_vec(),
                                                        );
                                                    } else {
                                                        send_http_response(
                                                            500,
                                                            default_headers.clone(),
                                                            "Unable to get chat invite link"
                                                                .to_string()
                                                                .as_bytes()
                                                                .to_vec(),
                                                        );
                                                    }
                                                } else {
                                                    send_http_response(
                                                        500,
                                                        default_headers.clone(),
                                                        "Unable to get chat invite link"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                }
                                            } else {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Chat ID"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        }
                                        "/orgs/:platform/bots" => {
                                            if message_json["url_params"]["platform"] == "telegram"
                                            {
                                                let Ok(token) = String::from_utf8(payload.bytes)
                                                else {
                                                    print_to_terminal(0, "orgs: no token for bot");
                                                    send_http_response(
                                                        400,
                                                        default_headers.clone(),
                                                        "Invalid JSON"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                    continue;
                                                };

                                                // Check if the bot already exists
                                                let response = send_and_await_response(
                                                    &Address {
                                                        node: our.node.clone(),
                                                        process: ProcessId::Name("http_client".to_string()),
                                                    },
                                                    &Request {
                                                        inherit: false,
                                                        expects_response: Some(5), // TODO evaluate timeout
                                                        ipc: Some(json!({
                                                            "method": "GET",
                                                            "uri": format!("https://api.telegram.org/bot{}/getMe", token),
                                                            "headers": {
                                                                "Content-Type": "application/json",
                                                            },
                                                        }).to_string()),
                                                        metadata: None,
                                                    },
                                                    None,
                                                );

                                                let bot: Option<TelegramBot> =
                                                    match get_response_info(response) {
                                                        (Some(ipc), Some(payload), _) => {
                                                            let json = serde_json::from_str::<
                                                                serde_json::Value,
                                                            >(
                                                                &ipc
                                                            )
                                                            .unwrap();
                                                            if json["status"]
                                                                .as_u64()
                                                                .unwrap_or_default()
                                                                < 300
                                                            {
                                                                match serde_json::from_slice::<
                                                                serde_json::Value,
                                                            >(
                                                                &payload.bytes
                                                            ) {
                                                                Ok(bot_json) => {
                                                                    Some(TelegramBot {
                                                                        id: bot_json["result"]["id"].as_u64().unwrap_or(0),
                                                                        is_bot: bot_json["result"]["is_bot"].as_bool().unwrap_or(false),
                                                                        first_name: bot_json["result"]["first_name"].as_str().unwrap_or("").to_string(),
                                                                        username: bot_json["result"]["username"].as_str().unwrap_or("").to_string(),
                                                                        can_join_groups: bot_json["result"]["can_join_groups"].as_bool().unwrap_or(false),
                                                                        can_read_all_group_messages: bot_json["result"]["can_read_all_group_messages"].as_bool().unwrap_or(false),
                                                                        supports_inline_queries: bot_json["result"]["supports_inline_queries"].as_bool().unwrap_or(false),
                                                                        token: token,
                                                                        chats: HashMap::new(),
                                                                    })
                                                                }
                                                                Err(_) => None,
                                                            }
                                                            } else {
                                                                None
                                                            }
                                                        }
                                                        _ => None,
                                                    };

                                                if let Some(bot) = bot {
                                                    let bot_id = bot.id.clone();
                                                    let bot_token = bot.token.clone();
                                                    state.telegram_bots
                                                        .insert(bot_id.clone(), bot.clone());
                                                    send_http_client_request(
                                                        our.node.clone(),
                                                        format!(
                                                            "https://api.telegram.org/bot{}/getUpdates",
                                                            bot_token
                                                        ),
                                                        "GET",
                                                        HashMap::new(),
                                                        Vec::new(),
                                                        Some(
                                                            json!({
                                                                "telegram_bot_id": bot_id
                                                            })
                                                            .to_string(),
                                                        ),
                                                    );
                                                    send_http_response(
                                                        201,
                                                        default_headers.clone(),
                                                        serde_json::to_string(&bot)
                                                            .unwrap_or_default()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                } else {
                                                    send_http_response(
                                                        500,
                                                        default_headers.clone(),
                                                        "Unable to create bot"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                }
                                            } else {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Bot Platform"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        }
                                        _ => send_http_response(
                                            404,
                                            default_headers.clone(),
                                            "Not Found".to_string().as_bytes().to_vec(),
                                        ),
                                    }
                                }
                                "PUT" => {
                                    let Some(payload) = get_payload() else {
                                        print_to_terminal(
                                            0,
                                            "orgs: no bytes in payload, skipping...",
                                        );
                                        send_http_response(
                                            400,
                                            default_headers.clone(),
                                            "No payload".to_string().as_bytes().to_vec(),
                                        );
                                        continue;
                                    };
                                    let body_json = match serde_json::from_slice(&payload.bytes) {
                                        Ok(v) => v,
                                        Err(_) => {
                                            print_to_terminal(0, "orgs: JSON is not valid");
                                            send_http_response(
                                                400,
                                                default_headers.clone(),
                                                "Invalid JSON".to_string().as_bytes().to_vec(),
                                            );
                                            continue;
                                        }
                                    };

                                    match path.as_str() {
                                        "/orgs" => {}
                                        _ => send_http_response(
                                            404,
                                            default_headers.clone(),
                                            "Not Found".to_string().as_bytes().to_vec(),
                                        ),
                                    }
                                }
                                "DELETE" => match path.as_str() {
                                    "/orgs" => {}
                                    "/orgs/:org_id/members" => {
                                        let username =
                                            query_params["username"].as_str().unwrap_or("");
                                        let org_id = match url_params["org_id"]
                                            .as_str()
                                            .unwrap_or("0")
                                            .parse::<u64>()
                                        {
                                            Ok(value) => value,
                                            Err(e) => {
                                                print_to_terminal(
                                                    1,
                                                    format!("orgs: failed to parse org_id: {}", e)
                                                        .as_str(),
                                                );
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Org ID"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                                continue;
                                            }
                                        };
                                        if let Some(org) = state.orgs.get_mut(&org_id) {
                                            modify_telegram_membership(
                                                org,
                                                our.node.clone(),
                                                &state.telegram_bots,
                                                &state.address_book,
                                                username.to_string(),
                                                "banChatMember",
                                            );
                                            modify_telegram_membership(
                                                org,
                                                our.node.clone(),
                                                &state.telegram_bots,
                                                &state.address_book,
                                                username.to_string(),
                                                "unbanChatMember",
                                            );
                                            org.members.remove(username);
                                            send_http_response(
                                                200,
                                                default_headers.clone(),
                                                "OK".to_string().as_bytes().to_vec(),
                                            );
                                        } else {
                                            send_http_response(
                                                400,
                                                default_headers.clone(),
                                                "Invalid Org ID".to_string().as_bytes().to_vec(),
                                            );
                                        }
                                    }
                                    "/orgs/:org_id/chats" => {
                                        let platform =
                                            query_params["platform"].as_str().unwrap_or("");
                                        let org_id = match url_params["org_id"]
                                            .as_str()
                                            .unwrap_or("0")
                                            .parse::<u64>()
                                        {
                                            Ok(value) => value,
                                            Err(e) => {
                                                print_to_terminal(
                                                    1,
                                                    format!("orgs: failed to parse org_id: {}", e)
                                                        .as_str(),
                                                );
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Org ID"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                                continue;
                                            }
                                        };
                                        if let Some(org) = state.orgs.get_mut(&org_id) {
                                            if let Some(chat) = org.chats.get(platform) {
                                                org.chats.remove(platform);
                                                send_http_response(
                                                    200,
                                                    default_headers.clone(),
                                                    "OK".to_string().as_bytes().to_vec(),
                                                );
                                            } else {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Chat Platform"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                            }
                                        } else {
                                            send_http_response(
                                                400,
                                                default_headers.clone(),
                                                "Invalid Org ID".to_string().as_bytes().to_vec(),
                                            );
                                        }
                                    }
                                    "/orgs/:platform/bots" => {
                                        let platform =
                                            url_params["platform"].as_str().unwrap_or("");
                                        let bot_id = match url_params["id"]
                                            .as_str()
                                            .unwrap_or("")
                                            .parse::<u64>()
                                        {
                                            Ok(value) => value,
                                            Err(e) => {
                                                print_to_terminal(
                                                    1,
                                                    format!("orgs: failed to parse bot_id: {}", e)
                                                        .as_str(),
                                                );
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Invalid Bot ID"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                );
                                                continue;
                                            }
                                        };
                                        // 1. Delete all chats in all orgs managed by this bot
                                        for org in state.orgs.values_mut() {
                                            let mut has_chat = false;
                                            if let Some(chat) = org.chats.get_mut(platform) {
                                                for bot in state.telegram_bots.values() {
                                                    if bot.chats.contains_key(&chat.id) {
                                                        has_chat = true;
                                                        break;
                                                    }
                                                }
                                            }
                                            if has_chat {
                                                org.chats.remove(platform);
                                            }
                                        }
                                        // 2. Delete the bot from bots
                                        state.telegram_bots.remove(&bot_id);
                                    }
                                    _ => {
                                        send_http_response(
                                            404,
                                            default_headers.clone(),
                                            "Not Found".to_string().as_bytes().to_vec(),
                                        );
                                        continue;
                                    }
                                },
                                _ => {
                                    send_http_response(
                                        404,
                                        default_headers.clone(),
                                        "Not Found".to_string().as_bytes().to_vec(),
                                    );
                                    continue;
                                }
                            }
                        }
                    } else {
                        // Handling WS messages here
                        if let Some(payload) = get_payload() {
                            // TODO: make a message system here
                            if let Ok(json) =
                                serde_json::from_slice::<serde_json::Value>(&payload.bytes)
                            {
                                print_to_terminal(0, format!("JSON: {}", json).as_str());
                                // Handle the websocket messages
                            }
                        }
                    }
                }
                Message::Response((response, context)) => {
                    if source.process == ProcessId::Name("http_client".to_string()) {
                        let Some(bot_id_string) = context else {
                            print_to_terminal(0, "orgs: got response without context");
                            continue;
                        };

                        let Ok(context) = serde_json::from_str::<serde_json::Value>(&bot_id_string)
                        else {
                            print_to_terminal(0, "orgs: context is not valid JSON");
                            continue;
                        };

                        let telegram_bot_id = context["telegram_bot_id"].as_u64().unwrap_or(0);

                        if telegram_bot_id != 0 {
                            let Some(payload) = get_payload() else {
                                print_to_terminal(
                                    0,
                                    "orgs: no bytes in response payload, skipping...",
                                );
                                continue;
                            };

                            let json =
                                match serde_json::from_slice::<serde_json::Value>(&payload.bytes) {
                                    Ok(v) => v,
                                    Err(_) => {
                                        print_to_terminal(0, "orgs: JSON is not valid");
                                        continue;
                                    }
                                };

                            print_to_terminal(1, format!("orgs: response JSON {}", json).as_str());

                            let update_id = handle_telegram_update(
                                our.node.clone(),
                                telegram_bot_id,
                                json,
                                &mut state.orgs,
                                &mut state.telegram_bots,
                                &mut state.address_book,
                            );
                            let token = state.telegram_bots.get(&telegram_bot_id).unwrap().token.clone();

                            let uri = match update_id {
                                Some(id) => format!(
                                    "https://api.telegram.org/bot{}/getUpdates?offset={}",
                                    token,
                                    id + 1
                                ),
                                None => format!("https://api.telegram.org/bot{}/getUpdates", token),
                            };

                            if state.telegram_bots.contains_key(&telegram_bot_id) {
                                send_http_client_request(
                                    our.node.clone(),
                                    uri,
                                    "GET",
                                    HashMap::new(),
                                    Vec::new(),
                                    Some(
                                        serde_json::json!({
                                            "telegram_bot_id": telegram_bot_id
                                        })
                                        .to_string(),
                                    ),
                                );
                            }
                        }
                    } else if source.process == ProcessId::Name("orgs".to_string()) {
                        if let Some(json) = response.ipc {
                            let message_json: serde_json::Value = match serde_json::from_str(&json)
                            {
                                Ok(v) => v,
                                Err(_) => {
                                    print_to_terminal(0, "orgs: failed to parse ipc JSON");
                                    continue;
                                }
                            };

                            if message_json["action"] == "get_contact_info" {
                                let Some(payload) = get_payload() else {
                                    print_to_terminal(
                                        0,
                                        "orgs: no bytes in response payload, skipping...",
                                    );
                                    continue;
                                };

                                let contact_info =
                                    match serde_json::from_slice::<Contact>(&payload.bytes) {
                                        Ok(v) => v,
                                        Err(_) => {
                                            print_to_terminal(0, "orgs: failed to parse contact");
                                            continue;
                                        }
                                    };

                                state.address_book.insert(source.node.clone(), contact_info.clone());
                            }
                        }
                    } else {
                        print_to_terminal(0, "orgs: got unexpected response");
                        continue;
                    }
                }
            }
        }
    }
}
