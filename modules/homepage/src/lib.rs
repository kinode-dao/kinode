cargo_component_bindings::generate!();

use bindings::component::uq_process::types::*;
use bindings::{
    get_payload, print_to_terminal, receive, send_request, send_requests, send_response, Guest,
};
use serde_json::json;

#[allow(dead_code)]
mod process_lib;

struct Component;

const HOME_PAGE: &str = include_str!("home.html");

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
            ipc: json!({
                "BindPath": {
                    "path": path,
                    "authenticated": authenticated,
                    "local_only": false
                }
            })
            .to_string()
            .as_bytes()
            .to_vec(),
            metadata: None,
        },
        None,
        None,
    )
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "homepage: start");

        let bindings_address = Address {
            node: our.node.clone(),
            process: ProcessId::from_str("http_server:sys:uqbar").unwrap(),
        };

        // <address, request, option<context>, option<payload>>
        let http_endpoint_binding_requests: [(Address, Request, Option<Context>, Option<Payload>);
            1] = [generate_http_binding(bindings_address.clone(), "/", true)];
        send_requests(&http_endpoint_binding_requests);

        loop {
            let Ok((_source, message)) = receive() else {
                print_to_terminal(0, "homepage: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                print_to_terminal(0, &format!("homepage: got unexpected message: {:?}", message));
                continue;
            };

            let message_json: serde_json::Value = match serde_json::from_slice(&request.ipc) {
                Ok(v) => v,
                Err(_) => {
                    print_to_terminal(1, "homepage: failed to parse ipc JSON, skipping");
                    continue;
                }
            };

            if message_json["path"] == "/" && message_json["method"] == "GET" {
                print_to_terminal(1, "homepage: sending response");

                send_response(
                    &Response {
                        inherit: false,
                        ipc: serde_json::json!({
                            "action": "response",
                            "status": 200,
                            "headers": {
                                "Content-Type": "text/html",
                            },
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("text/html".to_string()),
                        bytes: HOME_PAGE
                            .replace("${our}", &our.node)
                            .to_string()
                            .as_bytes()
                            .to_vec(),
                    }),
                );
            } else if message_json["path"].is_string() {
                send_response(
                    &Response {
                        inherit: false,
                        ipc: json!({
                            "action": "response",
                            "status": 404,
                            "headers": {
                                "Content-Type": "text/html",
                            },
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("text/html".to_string()),
                        bytes: "Not Found".to_string().as_bytes().to_vec(),
                    }),
                );
            } else if message_json["hello"] == "world" {
                send_response(
                    &Response {
                        inherit: false,
                        ipc: serde_json::json!({
                            "hello": "to you too"
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("application/json".to_string()),
                        bytes: serde_json::json!({
                            "hello": "to you too"
                        })
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                    }),
                );
            } else {
                if let Some(payload) = get_payload() {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&payload.bytes) {
                        print_to_terminal(1, format!("JSON: {}", json).as_str());
                        if json["message"] == "ping" {
                            // WebSocket pushes are sent as requests
                            send_request(
                                &Address {
                                    node: our.node.clone(),
                                    process: ProcessId::from_str("encryptor:sys:uqbar").unwrap(),
                                },
                                &Request {
                                    inherit: false,
                                    expects_response: None,
                                    ipc: serde_json::json!({
                                        "EncryptAndForwardAction": {
                                            "channel_id": "homepage",
                                            "forward_to": {
                                                "node": our.node.clone(),
                                                "process": {
                                                    "process_name": "http_server",
                                                    "package_name": "sys",
                                                    "publisher_node": "uqbar"
                                                }
                                            }, // node, process
                                            "json": Some(serde_json::json!({ // this is the JSON to forward
                                                "WebSocketPush": {
                                                    "target": {
                                                        "node": our.node.clone(),
                                                        "id": "homepage", // If the message passed in an ID then we could send to just that ID
                                                    }
                                                }
                                            })),
                                        }

                                    })
                                    .to_string()
                                    .as_bytes()
                                    .to_vec(),
                                    metadata: None,
                                },
                                None,
                                Some(&Payload {
                                    mime: Some("application/json".to_string()),
                                    bytes: serde_json::json!({
                                        "pong": true
                                    })
                                    .to_string()
                                    .as_bytes()
                                    .to_vec(),
                                }),
                            );
                        }
                    }
                }
            }
        }
    }
}
