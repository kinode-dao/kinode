cargo_component_bindings::generate!();

use std::collections::HashMap;
use serde_json::json;
use serde::{Serialize, Deserialize};

use bindings::{print_to_terminal, receive, send_requests, send_request, send_response, get_payload, Guest};
use bindings::component::uq_process::types::*;

mod process_lib;

const PROXY_HOME_PAGE: &str = include_str!("http_proxy.html");

struct Component;

#[derive(Debug, Serialize, Deserialize)]
pub enum FileSystemAction {
    Read,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileSystemRequest {
    pub uri_string: String,
    pub action: FileSystemAction,
}

fn send_http_response(
    status: u16,
    headers: HashMap<String, String>,
    payload_bytes: Vec<u8>,
) {
    send_response(
        &Response {
            ipc: Some(serde_json::json!({
                "status": status,
                "headers": headers,
            }).to_string()),
            metadata: None,
        },
        Some(&Payload {
            mime: Some("text/html".to_string()),
            bytes: payload_bytes,
        })
    )
}

fn send_not_found () {
    send_http_response(404,HashMap::new(),"Not Found".to_string().as_bytes().to_vec())
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(1, "http_proxy: start");

        let mut registrations: HashMap<String, String> = HashMap::new();

        let bindings_address = Address {
            node: our.node.clone(),
            process: ProcessId::Name("http_bindings".to_string()),
        };

        // <address, request, option<context>, option<payload>>
        let http_endpoint_binding_requests: [(Address, Request, Option<Context>, Option<Payload>); 5] = [
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(serde_json::json!({
                        "action": "bind-app",
                        "path": "/http-proxy",
                        "authenticated": true,
                        "app": "http_proxy",
                    }).to_string()),
                    metadata: None,
                },
                None,
                None
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(serde_json::json!({
                        "action": "bind-app",
                        "path": "/http-proxy/static/*",
                        "authenticated": true,
                        "app": "http_proxy",
                    }).to_string()),
                    metadata: None,
                },
                None,
                None
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(serde_json::json!({
                        "action": "bind-app",
                        "path": "/http-proxy/list",
                        "app": "http_proxy",
                    }).to_string()),
                    metadata: None,
                },
                None,
                None
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(serde_json::json!({
                        "action": "bind-app",
                        "path": "/http-proxy/register",
                        "app": "http_proxy",
                    }).to_string()),
                    metadata: None,
                },
                None,
                None
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(serde_json::json!({
                        "action": "bind-app",
                        "path": "/http-proxy/serve/:username/*",
                        "app": "http_proxy",
                    }).to_string()),
                    metadata: None,
                },
                None,
                None
            ),
        ];
        send_requests(&http_endpoint_binding_requests);

        loop {
            let Ok((_source, message)) = receive() else {
                print_to_terminal(0, "http_proxy: got network error");
                let mut headers = HashMap::new();
                headers.insert("Content-Type".to_string(), "text/html".to_string());
                send_http_response(503, headers, format!("<h1>Node Offline</h1>").as_bytes().to_vec());
                continue;
            };
            let Message::Request(request) = message else {
                print_to_terminal(0, "http_proxy: got unexpected message");
                continue;
            };

            let Some(json) = request.ipc else {
                print_to_terminal(1, "http_proxy: no ipc json");
                continue;
            };

            let message_json: serde_json::Value = match serde_json::from_str(&json) {
                Ok(v) => v,
                Err(_) => {
                    print_to_terminal(1, "http_proxy: failed to parse ipc JSON, skipping");
                    continue;
                },
            };

            print_to_terminal(1, format!("http_proxy: got request: {}", message_json).as_str());

            if message_json["path"] == "/http-proxy" && message_json["method"] == "GET" {
                send_response(
                    &Response {
                        ipc: Some(serde_json::json!({
                            "action": "response",
                            "status": 200,
                            "headers": {
                                "Content-Type": "text/html",
                            },
                        }).to_string()),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("text/html".to_string()),
                        bytes: PROXY_HOME_PAGE
                            .replace("${our}", &our.node)
                            .as_bytes()
                            .to_vec(),
                    }),
                );
            } else if message_json["path"] == "/http-proxy/list" && message_json["method"] == "GET" {
                send_response(
                    &Response {
                        ipc: Some(serde_json::json!({
                            "action": "response",
                            "status": 200,
                            "headers": {
                                "Content-Type": "application/json",
                            },
                        }).to_string()),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::json!({"registrations": registrations})
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                    }),
                );
            } else if message_json["path"] == "/http-proxy/register" && message_json["method"] == "POST" {
                let mut status = 204;

                let Some(payload) = get_payload() else {
                    print_to_terminal(1, "/http-proxy/register POST with no bytes");
                    continue;
                };

                let body: serde_json::Value = match serde_json::from_slice(&payload.bytes) {
                    Ok(s) => s,
                    Err(e) => {
                        print_to_terminal(1, format!("Bad body format: {}", e).as_str());
                        continue;
                    }
                };

                let username = body["username"].as_str().unwrap_or("");

                print_to_terminal(1, format!("Register proxy for: {}", username).as_str());

                if !username.is_empty() {
                    registrations.insert(username.to_string(), "foo".to_string());
                } else {
                    status = 400;
                }

                send_response(
                    &Response {
                        ipc: Some(serde_json::json!({
                            "action": "response",
                            "status": status,
                            "headers": {
                                "Content-Type": "text/html",
                            },
                        }).to_string()),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("text/html".to_string()),
                        bytes: (if status == 400 { "Bad Request" } else { "Success" })
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                    }),
                );
            } else if message_json["path"] == "/http-proxy/register" && message_json["method"] == "DELETE" {
                print_to_terminal(1, "HERE IN /http-proxy/register to delete something");
                let username = message_json["query_params"]["username"].as_str().unwrap_or("");

                let mut status = 204;

                if !username.is_empty() {
                    registrations.remove(username);
                } else {
                    status = 400;
                }

                send_response(
                    &Response {
                        ipc: Some(serde_json::json!({
                            "action": "response",
                            "status": status,
                            "headers": {
                                "Content-Type": "text/html",
                            },
                        }).to_string()),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("text/html".to_string()),
                        bytes: (if status == 400 { "Bad Request" } else { "Success" })
                            .to_string()
                            .as_bytes()
                            .to_vec()
                    }),
                );
            } else if message_json["path"] == "/http-proxy/serve/:username/*" {
                let username = message_json["url_params"]["username"].as_str().unwrap_or("");
                let raw_path = message_json["raw_path"].as_str().unwrap_or("");
                print_to_terminal(1, format!("proxy for user: {}", username).as_str());

                if username.is_empty() || raw_path.is_empty() {
                    send_not_found();
                } else if !registrations.contains_key(username) {
                    send_response(
                        &Response {
                            ipc: Some(json!({
                                "action": "response",
                                "status": 403,
                                "headers": {
                                    "Content-Type": "text/html",
                                },
                            }).to_string()),
                            metadata: None,
                        },
                        Some(&Payload {
                            mime: Some("text/html".to_string()),
                            bytes: "Not Authorized"
                                .to_string()
                                .as_bytes()
                                .to_vec(),
                        }),
                    );
                } else {
                    let path_parts: Vec<&str> = raw_path.split('/').collect();
                    let mut proxied_path = "/".to_string();

                    if let Some(pos) = path_parts.iter().position(|&x| x == "serve") {
                        proxied_path = format!("/{}", path_parts[pos+2..].join("/"));
                        print_to_terminal(1, format!("Path to proxy: {}", proxied_path).as_str());
                    }

                    let payload = get_payload();

                    send_request(
                        &Address {
                            node: username.into(),
                            process: ProcessId::Name("http_bindings".to_string()),
                        },
                        &Request {
                            inherit: true,
                            expects_response: None,
                            ipc: Some(json!({
                                "action": "request",
                                "method": message_json["method"],
                                "path": proxied_path,
                                "headers": message_json["headers"],
                                "proxy_path": raw_path,
                                "query_params": message_json["query_params"],
                            }).to_string()),
                            metadata: None,
                        },
                        None,
                        payload.as_ref(),
                    );
                }
            } else {
                send_not_found();
            }
        }
    }
}
