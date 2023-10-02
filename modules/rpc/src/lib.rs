cargo_component_bindings::generate!();

use bindings::component::uq_process::types::*;
use bindings::{
    get_capabilities, get_capability, get_payload, print_to_terminal, receive,
    send_and_await_response, send_request, send_requests, send_response, Guest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
extern crate base64;

mod process_lib;

struct Component;

#[derive(Debug, Serialize, Deserialize)]
struct RpcMessage {
    pub node: String,
    pub process: String,
    pub inherit: Option<bool>,
    pub expects_response: Option<u64>, // always false?
    pub ipc: Option<String>,
    pub metadata: Option<String>,
    pub context: Option<String>,
    pub mime: Option<String>,
    pub data: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilitiesTransfer {
    pub destination_node: String,
    pub destination_process: String,
    pub node: String,
    pub process: String,
    pub params: String,
}

// curl http://localhost:8080/rpc/message -H 'content-type: application/json' -d '{"node": "hosted", "process": "vfs", "inherit": false, "expects_response": null, "ipc": "{\"New\": {\"identifier\": \"foo\"}}", "metadata": null, "context": null, "mime": null, "data": null}'

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

const RPC_PAGE: &str = include_str!("rpc.html");

fn binary_encoded_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u8).collect()
}

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "RPC: start");

        let bindings_address = Address {
            node: our.node.clone(),
            process: ProcessId::Name("http_bindings".to_string()),
        };

        // <address, request, option<context>, option<payload>>
        let http_endpoint_binding_requests: [(Address, Request, Option<Context>, Option<Payload>);
            3] = [
            // (
            //     bindings_address.clone(),
            //     Request {
            //         inherit: false,
            //         expects_response: None,
            //         ipc: Some(json!({
            //             "action": "bind-app",
            //             "path": "/rpc",
            //             "app": "rpc",
            //             "local_only": true,
            //         }).to_string()),
            //         metadata: None,
            //     },
            //     None,
            //     None
            // ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(
                        json!({
                            "action": "bind-app",
                            "path": "/rpc/message",
                            "app": "rpc",
                            "local_only": true,
                        })
                        .to_string(),
                    ),
                    metadata: None,
                },
                None,
                None,
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(
                        json!({
                            "action": "bind-app",
                            "path": "/rpc/capabilities",
                            "app": "rpc",
                            "local_only": true,
                        })
                        .to_string(),
                    ),
                    metadata: None,
                },
                None,
                None,
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(
                        json!({
                            "action": "bind-app",
                            "path": "/rpc/capabilities/transfer",
                            "app": "rpc",
                            "local_only": true,
                        })
                        .to_string(),
                    ),
                    metadata: None,
                },
                None,
                None,
            ),
        ];
        send_requests(&http_endpoint_binding_requests);

        loop {
            let Ok((_source, message)) = receive() else {
                print_to_terminal(0, "rpc: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                print_to_terminal(0, "rpc: got unexpected message");
                continue;
            };

            if let Some(json) = request.ipc {
                print_to_terminal(1, format!("rpc: JSON {}", json).as_str());
                let message_json: serde_json::Value = match serde_json::from_str(&json) {
                    Ok(v) => v,
                    Err(_) => {
                        print_to_terminal(1, "rpc: failed to parse ipc JSON, skipping");
                        continue;
                    }
                };

                print_to_terminal(1, "rpc: parsed ipc JSON");

                let path = message_json["path"].as_str().unwrap_or("");
                let method = message_json["method"].as_str().unwrap_or("");

                let mut default_headers = HashMap::new();
                default_headers.insert("Content-Type".to_string(), "text/html".to_string());
                // Handle incoming http
                print_to_terminal(1, format!("rpc: path {}", path).as_str());
                print_to_terminal(1, format!("rpc: method {}", method).as_str());
                match method {
                    "GET" => match path {
                        "/rpc" => {
                            send_response(
                                &Response {
                                    ipc: Some(
                                        json!({
                                            "action": "response",
                                            "status": 200,
                                            "headers": {
                                                "Content-Type": "text/html",
                                            },
                                        })
                                        .to_string(),
                                    ),
                                    metadata: None,
                                },
                                Some(&Payload {
                                    mime: Some("text/html".to_string()),
                                    bytes: RPC_PAGE
                                        .replace("${our}", &our.node)
                                        .to_string()
                                        .as_bytes()
                                        .to_vec(),
                                }),
                            );
                        }
                        "/rpc/capabilities" => {
                            let capabilities = get_capabilities();
                            let caps = capabilities
                                .iter()
                                .map(|cap| {
                                    let process = match &cap.issuer.process {
                                        ProcessId::Name(name) => name.clone(),
                                        ProcessId::Id(id) => id.to_string(),
                                    };
                                    json!({
                                        "issuer": {
                                            "node": cap.issuer.node.clone(),
                                            "process": process,
                                        },
                                        "params": cap.params.clone(),
                                    })
                                })
                                .collect::<Vec<serde_json::Value>>();

                            send_http_response(
                                200,
                                default_headers.clone(),
                                json!(caps).to_string().as_bytes().to_vec(),
                            );
                            continue;
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
                    "POST" => match path {
                        "/rpc/message" => {
                            let Some(payload) = get_payload() else {
                                print_to_terminal(1, "rpc: no bytes in payload, skipping...");
                                send_http_response(
                                    400,
                                    default_headers.clone(),
                                    "No payload".to_string().as_bytes().to_vec(),
                                );
                                continue;
                            };

                            let body_json: RpcMessage =
                                match serde_json::from_slice::<RpcMessage>(&payload.bytes) {
                                    Ok(v) => v,
                                    Err(_) => {
                                        print_to_terminal(
                                            1,
                                            &format!(
                                                "rpc: JSON is not valid RpcMessage: {:?}",
                                                serde_json::from_slice::<serde_json::Value>(
                                                    &payload.bytes
                                                )
                                            ),
                                        );
                                        send_http_response(
                                            400,
                                            default_headers.clone(),
                                            "JSON is not valid RpcMessage"
                                                .to_string()
                                                .as_bytes()
                                                .to_vec(),
                                        );
                                        continue;
                                    }
                                };

                            let payload =
                                match base64::decode(&body_json.data.unwrap_or("".to_string())) {
                                    Ok(bytes) => Some(Payload {
                                        mime: body_json.mime,
                                        bytes,
                                    }),
                                    Err(_) => None,
                                };

                            let caps = get_capabilities();
                            print_to_terminal(
                                0,
                                format!("rpc: got capabilities {:?}", caps).as_str(),
                            );

                            let result = send_and_await_response(
                                &Address {
                                    node: body_json.node,
                                    process: ProcessId::Name(body_json.process),
                                },
                                &Request {
                                    inherit: false,
                                    expects_response: Some(5), // TODO evaluate timeout
                                    ipc: body_json.ipc,
                                    metadata: body_json.metadata,
                                },
                                payload.as_ref(),
                            );

                            match result {
                                Ok((_source, message)) => {
                                    let Message::Response((response, _context)) = message
                                    else {
                                        print_to_terminal(
                                            1,
                                            "rpc: got unexpected response to message",
                                        );
                                        send_http_response(
                                            500,
                                            default_headers,
                                            "Invalid Internal Response"
                                                .to_string()
                                                .as_bytes()
                                                .to_vec(),
                                        );
                                        continue;
                                    };

                                let (mime, data) = match get_payload() {
                                    Some(p) => {
                                        let mime = match p.mime {
                                            Some(mime) => mime,
                                            None => "application/octet-stream".to_string(),
                                        };
                                        let bytes = p.bytes;

                                            (mime, base64::encode(bytes))
                                        }
                                        None => ("".to_string(), "".to_string()),
                                    };

                                    let body = json!({
                                        "ipc": response.ipc,
                                        "payload": {
                                            "mime": mime,
                                            "data": data,
                                        },
                                    })
                                    .to_string()
                                    .as_bytes()
                                    .to_vec();

                                    send_http_response(200, default_headers.clone(), body);
                                    continue;
                                }
                                Err(error) => {
                                    print_to_terminal(1, "rpc: error coming back");
                                    send_http_response(
                                        500,
                                        default_headers.clone(),
                                        "Network Error".to_string().as_bytes().to_vec(),
                                    );
                                    continue;
                                }
                            }
                        }
                        "/rpc/capabilities/transfer" => {
                            let Some(payload) = get_payload() else {
                                print_to_terminal(1, "rpc: no bytes in payload, skipping...");
                                send_http_response(
                                    400,
                                    default_headers.clone(),
                                    "No payload".to_string().as_bytes().to_vec(),
                                );
                                continue;
                            };
                            let body_json: CapabilitiesTransfer = match serde_json::from_slice::<
                                CapabilitiesTransfer,
                            >(
                                &payload.bytes
                            ) {
                                Ok(v) => v,
                                Err(_) => {
                                    print_to_terminal(
                                        1,
                                        &format!(
                                            "rpc: JSON is not valid CapabilitiesTransfer: {:?}",
                                            serde_json::from_slice::<serde_json::Value>(
                                                &payload.bytes
                                            )
                                        ),
                                    );
                                    send_http_response(
                                        400,
                                        default_headers.clone(),
                                        "JSON is not valid CapabilitiesTransfer"
                                            .to_string()
                                            .as_bytes()
                                            .to_vec(),
                                    );
                                    continue;
                                }
                            };

                            print_to_terminal(
                                0,
                                format!("rpc: node {:?}", body_json.node).as_str(),
                            );
                            print_to_terminal(
                                0,
                                format!("rpc: process {:?}", body_json.process).as_str(),
                            );
                            print_to_terminal(
                                0,
                                format!("rpc: params {:?}", body_json.params).as_str(),
                            );

                            let capability = get_capability(
                                &Address {
                                    node: body_json.node,
                                    process: ProcessId::Name(body_json.process),
                                },
                                &body_json.params,
                            );

                            print_to_terminal(
                                0,
                                format!("rpc: got capability {:?}", capability).as_str(),
                            );

                            match capability {
                                Some(capability) => {
                                    let process = match capability.issuer.process {
                                        ProcessId::Name(name) => name,
                                        ProcessId::Id(id) => id.to_string(),
                                    };
                                    send_request(
                                        &Address {
                                            node: body_json.destination_node,
                                            process: ProcessId::Name(body_json.destination_process),
                                        },
                                        &Request {
                                            inherit: false,
                                            expects_response: None,
                                            ipc: Some(
                                                json!({
                                                    "action": "transfer_capability",
                                                    "info": {
                                                        "issuer": {
                                                            "node": capability.issuer.node,
                                                            "process": process,
                                                        },
                                                        "params": capability.params,
                                                    }
                                                })
                                                .to_string(),
                                            ),
                                            metadata: None,
                                        },
                                        None,
                                        Some(&Payload {
                                            mime: Some("bytes".to_string()),
                                            bytes: capability.signature,
                                        }),
                                    );

                                    send_http_response(
                                        200,
                                        default_headers.clone(),
                                        "Success".to_string().as_bytes().to_vec(),
                                    );
                                }
                                None => send_http_response(
                                    404,
                                    default_headers.clone(),
                                    "Not Found".to_string().as_bytes().to_vec(),
                                ),
                            }
                            continue;
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
                            405,
                            default_headers.clone(),
                            "Method Not Allowed".to_string().as_bytes().to_vec(),
                        );
                        continue;
                    }
                }
            }
        }
    }
}
