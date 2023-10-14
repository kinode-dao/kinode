cargo_component_bindings::generate!();

use alloy_primitives::FixedBytes;
use alloy_sol_types::{sol, SolEvent};
use bindings::component::uq_process::types::*;
use bindings::{print_to_terminal, receive, send_request, send_response, UqProcess};
use hex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::string::FromUtf8Error;

#[allow(dead_code)]
mod process_lib;

struct Component;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, QnsUpdate>,
    // last block we read from
    block: u64,
}

#[derive(Debug, Serialize, Deserialize)]
enum AllActions {
    EventSubscription(EthEvent),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EthEvent {
    address: String,
    block_hash: String,
    block_number: String,
    data: String,
    log_index: String,
    removed: bool,
    topics: Vec<String>,
    transaction_hash: String,
    transaction_index: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetActions {
    QnsUpdate(QnsUpdate),
    QnsBatchUpdate(Vec<QnsUpdate>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QnsUpdate {
    pub name: String, // actual username / domain name
    pub owner: String,
    pub node: String, // hex namehash of node
    pub public_key: String,
    pub ip: String,
    pub port: u16,
    pub routers: Vec<String>,
}

sol! {
    event WsChanged(
        uint256 indexed node,
        uint96 indexed protocols,
        bytes32 publicKey,
        uint32 ip,
        uint16 port,
        bytes32[] routers
    );

    event NodeRegistered(uint256 indexed node, bytes name);
}

fn subscribe_to_qns(from_block: u64) -> String {
    json!({
        "SubscribeEvents": {
            "addresses": [
                // QNSRegistry on sepolia
                "0x9e5ed0e7873E0d7f10eEb6dE72E87fE087A12776",
            ],
            "from_block": from_block,
            "to_block": null,
            "events": [
                "NodeRegistered(uint256,bytes)",
                "WsChanged(uint256,uint96,bytes32,uint32,uint16,bytes32[])",
            ],
            "topic1": null,
            "topic2": null,
            "topic3": null,
        }
    })
    .to_string()
}

impl UqProcess for Component {
    fn init(our: Address) {
        let mut state: State = State {
            names: HashMap::new(),
            nodes: HashMap::new(),
            block: 1,
        };

        // if we have state, load it in
        match process_lib::get_state::<State>() {
            Some(s) => {
                state = s;
            }
            None => {}
        }

        bindings::print_to_terminal(
            0,
            &format!("qns_indexer: starting at block {}", state.block),
        );

        // shove all state into net::net
        send_request(
            &Address {
                node: our.node.clone(),
                process: ProcessId::from_str("net:sys:uqbar").unwrap(),
            },
            &Request {
                inherit: false,
                expects_response: None,
                metadata: None,
                ipc: Some(
                    serde_json::to_string(&NetActions::QnsBatchUpdate(
                        state.nodes.values().cloned().collect::<Vec<_>>(),
                    ))
                    .unwrap(),
                ),
            },
            None,
            None,
        );

        let _ = send_request(
            &Address {
                node: our.node.clone(),
                process: ProcessId::from_str("eth_rpc:sys:uqbar").unwrap(),
            },
            &Request {
                inherit: false,            // TODO what
                expects_response: Some(5), // TODO evaluate
                metadata: None,
                // -1 because there could be other events in the last processed block
                ipc: Some(subscribe_to_qns(state.block - 1)),
            },
            None,
            None,
        );

        let http_bindings_process_id_str = "http_bindings:http_bindings:uqbar";
        let http_bindings_process_id = ProcessId::from_str(http_bindings_process_id_str).unwrap();

        let _register_endpoint = send_request(
            &Address {
                node: our.node.clone(),
                process: http_bindings_process_id.clone(),
            },
            &Request {
                inherit: false,
                expects_response: None,
                metadata: None,
                ipc: Some(
                    serde_json::json!({
                        "action": "bind-app",
                        "path": "/qns-indexer/node/:name",
                        "app": "qns_indexer",
                        "authenticated": true,
                    })
                    .to_string(),
                ),
            },
            None,
            None,
        );

        loop {
            let Ok((source, message)) = receive() else {
                print_to_terminal(0, "qns_indexer: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                // TODO we should store the subscription ID for eth_rpc
                // incase we want to cancel/reset it
                // print_to_terminal(0, "qns_indexer: got response");
                continue;
            };

            if source.process.to_string() == http_bindings_process_id_str {
                if let Ok(ipc_json) = serde_json::from_str::<serde_json::Value>(
                    &request.ipc.clone().unwrap_or_default(),
                ) {
                    if ipc_json["path"].as_str().unwrap_or_default() == "/qns-indexer/node/:name" {
                        if let Some(name) = ipc_json["url_params"]["name"].as_str() {
                            if let Some(node) = state.nodes.get(name) {
                                send_response(
                                    &Response {
                                        ipc: Some(
                                            serde_json::json!({
                                                "status": 200,
                                                "headers": {
                                                    "Content-Type": "application/json",
                                                },
                                            })
                                            .to_string(),
                                        ),
                                        metadata: None,
                                    },
                                    Some(&Payload {
                                        mime: Some("application/json".to_string()),
                                        bytes: serde_json::to_string(&node)
                                            .unwrap()
                                            .as_bytes()
                                            .to_vec(),
                                    }),
                                );
                                continue;
                            }
                        }
                    }
                }
                send_response(
                    &Response {
                        ipc: Some(
                            serde_json::json!({
                                "status": 404,
                                "headers": {
                                    "Content-Type": "application/json",
                                },
                            })
                            .to_string(),
                        ),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("application/json".to_string()),
                        bytes: "Not Found".to_string().as_bytes().to_vec(),
                    }),
                );
                continue;
            }

            let Ok(msg) = serde_json::from_str::<AllActions>(request.ipc.as_ref().unwrap()) else {
                print_to_terminal(0, &format!("qns_indexer: got invalid message: {}", request.ipc.unwrap_or_default()));
                continue;
            };

            match msg {
                // Probably more message types later...maybe not...
                AllActions::EventSubscription(e) => {
                    state.block = hex_to_u64(&e.block_number).unwrap();
                    match decode_hex(&e.topics[0].clone()) {
                        NodeRegistered::SIGNATURE_HASH => {
                            // bindings::print_to_terminal(0, format!("qns_indexer: got NodeRegistered event: {:?}", e).as_str());

                            let node       = &e.topics[1];
                            let decoded    = NodeRegistered::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                            let Ok(name) = dnswire_decode(decoded.0.clone()) else {
                                bindings::print_to_terminal(0, &format!("qns_indexer: failed to decode name: {:?}", decoded.0));
                                continue;
                            };

                            state.names.insert(node.to_string(), name);
                        }
                        WsChanged::SIGNATURE_HASH => {
                            let node       = &e.topics[1];
                            let decoded     = WsChanged::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                            let public_key  = hex::encode(decoded.0);
                            let ip = decoded.1;
                            let port = decoded.2;
                            let routers_raw = decoded.3;
                            let routers: Vec<String> = routers_raw
                                .iter()
                                .map(|r| {
                                    let key = hex::encode(r);
                                    match state.names.get(&key) {
                                        Some(name) => name.clone(),
                                        None => format!("0x{}", key), // TODO it should actually just panic here
                                    }
                                })
                                .collect::<Vec<String>>();

                            let Some(name) = state.names.get(node) else {
                                bindings::print_to_terminal(0, &format!("qns_indexer: failed to find name for node during WsChanged: {:?}", node));
                                continue;
                            };

                            let update = QnsUpdate {
                                name: name.clone(),
                                owner: "0x".to_string(), // TODO or get rid of
                                node: node.clone(),
                                public_key: format!("0x{}", public_key),
                                ip: format!(
                                    "{}.{}.{}.{}",
                                    (ip >> 24) & 0xFF,
                                    (ip >> 16) & 0xFF,
                                    (ip >> 8) & 0xFF,
                                    ip & 0xFF
                                ),
                                port,
                                routers,
                            };

                            state.nodes.insert(name.clone(), update.clone());

                            send_request(
                                &Address {
                                    node: our.node.clone(),
                                    process: ProcessId::from_str("net:sys:uqbar").unwrap(),
                                },
                                &Request {
                                    inherit: false,
                                    expects_response: None,
                                    metadata: None,
                                    ipc: Some(
                                        serde_json::to_string(&NetActions::QnsUpdate(
                                            update.clone(),
                                        ))
                                        .unwrap(),
                                    ),
                                },
                                None,
                                None,
                            );
                        }
                        event => {
                            bindings::print_to_terminal(
                                0,
                                format!("qns_indexer: got unknown event: {:?}", event).as_str(),
                            );
                        }
                    }
                }
            }

            process_lib::set_state::<State>(&state);
        }
    }
}

// helpers
// TODO these probably exist somewhere in alloy...not sure where though.
fn decode_hex(s: &str) -> FixedBytes<32> {
    // If the string starts with "0x", skip the prefix
    let hex_part = if s.starts_with("0x") { &s[2..] } else { s };

    let mut arr = [0_u8; 32];
    arr.copy_from_slice(&hex::decode(hex_part).unwrap()[0..32]);
    FixedBytes(arr)
}

fn decode_hex_to_vec(s: &str) -> Vec<u8> {
    // If the string starts with "0x", skip the prefix
    let hex_part = if s.starts_with("0x") { &s[2..] } else { s };

    hex::decode(hex_part).unwrap()
}

fn hex_to_u64(hex: &str) -> Result<u64, std::num::ParseIntError> {
    let without_prefix = if hex.starts_with("0x") {
        &hex[2..]
    } else {
        hex
    };
    u64::from_str_radix(without_prefix, 16)
}

fn dnswire_decode(wire_format_bytes: Vec<u8>) -> Result<String, FromUtf8Error> {
    let mut i = 0;
    let mut result = Vec::new();

    while i < wire_format_bytes.len() {
        let len = wire_format_bytes[i] as usize;
        if len == 0 {
            break;
        }
        let end = i + len + 1;
        let mut span = wire_format_bytes[i + 1..end].to_vec();
        span.push('.' as u8);
        result.push(span);
        i = end;
    }

    let flat: Vec<_> = result.into_iter().flatten().collect();

    let name = String::from_utf8(flat)?;

    // Remove the trailing '.' if it exists (it should always exist)
    if name.ends_with('.') {
        Ok(name[0..name.len()-1].to_string())
    } else {
        Ok(name)
    }
}
