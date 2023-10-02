cargo_component_bindings::generate!();

use bindings::component::uq_process::types::*;
use bindings::{print_to_terminal, receive, send_request, send_response, UqProcess};
use serde::{Deserialize, Serialize};
use serde_json::json;
use alloy_primitives::FixedBytes;
use alloy_sol_types::{sol, SolEvent};
use hex;
use std::collections::HashMap;

mod process_lib;

struct Component;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    // namehash to human readable name
    names: HashMap<String, String>,
    // human readable name to most recent on-chain routing information as json
    // NOTE: not every namehash will have a node registered
    nodes: HashMap<String, String>,
    // last block we read from
    block: u64,
}

#[derive(Debug, Serialize, Deserialize)]
enum AllActions {
    EventSubscription(EthEvent),
}

#[derive(Debug, Serialize, Deserialize)]
struct EthEvent {
    address: String,
    blockHash: String,
    blockNumber: String,
    data: String,
    logIndex: String,
    removed: bool,
    topics: Vec<String>,
    transactionHash: String,
    transactionIndex: String,
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
    }).to_string()
}

impl UqProcess for Component {
    fn init(our: Address) {
        bindings::print_to_terminal(0, "qns_indexer: start");

        let mut state: State = State {
            names: HashMap::new(),
            nodes: HashMap::new(),
            block: 1,
        };

        // if we have state, load it in
        match process_lib::get_state(our.node.clone()) {
            None => {},
            Some(p) => {
                match bincode::deserialize::<State>(&p.bytes) {
                    Err(e) => print_to_terminal(
                        0,
                        &format!("qns_indexer: failed to deserialize payload from fs: {}", e),
                    ),
                    Ok(s) => {
                        state = s;
                    },
                }
            },
        }

        // shove all state into net::net
        for (_, ipc) in state.nodes.iter() {
            send_request(
                &Address{
                    node: our.node.clone(),
                    process: ProcessId::Name("net".to_string()),
                },
                &Request{
                    inherit: false,
                    expects_response: None,
                    metadata: None,
                    ipc: Some(ipc.to_string()),
                },
                None,
                None,
            );
        }

        let event_sub_res = send_request(
                &Address{
                    node: our.node.clone(),
                    process: ProcessId::Name("eth_rpc".to_string()),
                },
                &Request{
                    inherit: false, // TODO what
                    expects_response: Some(5), // TODO evaluate
                    metadata: None,
                    // -1 because there could be other events in the last processed block
                    ipc: Some(subscribe_to_qns(state.block - 1)),
                },
                None,
                None,
        );

        let _register_endpoint = send_request(
            &Address{
                node: our.node.clone(),
                process: ProcessId::Name("http_bindings".to_string()),
            },
            &Request{
                inherit: false,
                expects_response: None,
                metadata: None,
                ipc: Some(serde_json::json!({
                    "action": "bind-app",
                    "path": "/qns-indexer/node/:name",
                    "app": "qns_indexer",
                    "authenticated": true,
                }).to_string()),
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

            if source.process == ProcessId::Name("http_bindings".to_string()) {
                if let Ok(ipc_json) = serde_json::from_str::<serde_json::Value>(&request.ipc.clone().unwrap_or_default()) {
                    if ipc_json["path"].as_str().unwrap_or_default() == "/qns-indexer/node/:name" {
                        if let Some(name) = ipc_json["url_params"]["name"].as_str() {
                            if let Some(node) = state.nodes.get(name) {
                                send_response(
                                    &Response {
                                        ipc: Some(serde_json::json!({
                                            "status": 200,
                                            "headers": {
                                                "Content-Type": "application/json",
                                            },
                                        }).to_string()),
                                        metadata: None,
                                    },
                                    Some(&Payload {
                                        mime: Some("application/json".to_string()),
                                        bytes: node.as_bytes().to_vec(),
                                    })
                                );
                                continue;
                            }
                        }
                    }
                }
                send_response(
                    &Response {
                        ipc: Some(serde_json::json!({
                            "status": 404,
                            "headers": {
                                "Content-Type": "application/json",
                            },
                        }).to_string()),
                        metadata: None,
                    },
                    Some(&Payload {
                        mime: Some("application/json".to_string()),
                        bytes: "Not Found".to_string().as_bytes().to_vec(),
                    })
                );
                continue;
            }

            let Ok(msg) = serde_json::from_str::<AllActions>(&request.ipc.unwrap_or_default()) else {
                print_to_terminal(0, "qns_indexer: got invalid message");
                continue;
            };

            match msg {
                // Probably more message types later...maybe not...
                AllActions::EventSubscription(e) => {
                    match decode_hex(&e.topics[0].clone()) {
                        NodeRegistered::SIGNATURE_HASH => {
                            // bindings::print_to_terminal(0, format!("qns_indexer: got NodeRegistered event: {:?}", e).as_str());

                            let node       = &e.topics[1];
                            let decoded    = NodeRegistered::decode_data(&decode_hex_to_vec(&e.data), true).unwrap();
                            let name = dnswire_decode(decoded.0);

                            // bindings::print_to_terminal(0, format!("qns_indexer: NODE1: {:?}", node).as_str());
                            // bindings::print_to_terminal(0, format!("qns_indexer: NAME: {:?}", name.to_string()).as_str());

                            state.names.insert(node.to_string(), name);
                            state.block = hex_to_u64(&e.blockNumber).unwrap();
                        }
                        WsChanged::SIGNATURE_HASH => {
                            // bindings::print_to_terminal(0, format!("qns_indexer: got WsChanged event: {:?}", e).as_str());

                            let node       = &e.topics[1];
                            // bindings::print_to_terminal(0, format!("qns_indexer: NODE2: {:?}", node.to_string()).as_str());
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

                            let name = state.names.get(node).unwrap();
                            // bindings::print_to_terminal(0, format!("qns_indexer: NAME: {:?}", name).as_str());
                            // bindings::print_to_terminal(0, format!("qns_indexer: DECODED: {:?}", decoded).as_str());
                            // bindings::print_to_terminal(0, format!("qns_indexer: PUB KEY: {:?}", public_key).as_str());
                            // bindings::print_to_terminal(0, format!("qns_indexer: IP PORT: {:?} {:?}", ip, port).as_str());
                            // bindings::print_to_terminal(0, format!("qns_indexer: ROUTERS: {:?}", routers).as_str());

                            let json_payload = json!({
                                "QnsUpdate": {
                                    "name": name,
                                    "owner": "0x", // TODO or get rid of
                                    "node": node,
                                    "public_key": format!("0x{}", public_key),
                                    "ip": format!(
                                        "{}.{}.{}.{}",
                                        (ip >> 24) & 0xFF,
                                        (ip >> 16) & 0xFF,
                                        (ip >> 8) & 0xFF,
                                        ip & 0xFF
                                    ),
                                    "port": port,
                                    "routers": routers,
                                }
                            }).to_string();

                            state.nodes.insert(name.clone(), json_payload.clone());

                            send_request(
                                &Address{
                                    node: our.node.clone(),
                                    process: ProcessId::Name("net".to_string()),
                                },
                                &Request{
                                    inherit: false,
                                    expects_response: None,
                                    metadata: None,
                                    ipc: Some(json_payload),
                                },
                                None,
                                None,
                            );
                        }
                        event => {
                            bindings::print_to_terminal(0, format!("qns_indexer: got unknown event: {:?}", event).as_str());
                        }
                    }
                }
            }

            process_lib::await_set_state(our.node.clone(), &state);
        }
    }
}

// helpers
// TODO these probably exist somewhere in alloy...not sure where though.
fn decode_hex(s: &str) -> FixedBytes<32> {
    // If the string starts with "0x", skip the prefix
    let hex_part = if s.starts_with("0x") {
        &s[2..]
    } else {
        s
    };

    let mut arr = [0_u8; 32];
    arr.copy_from_slice(&hex::decode(hex_part).unwrap()[0..32]);
    FixedBytes(arr)
}

fn decode_hex_to_vec(s: &str) -> Vec<u8> {
    // If the string starts with "0x", skip the prefix
    let hex_part = if s.starts_with("0x") {
        &s[2..]
    } else {
        s
    };

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

fn dnswire_decode(wire_format_bytes: Vec<u8>) -> String {
    let mut i = 0;
    let mut result = Vec::new();

    while i < wire_format_bytes.len() {
        let len = wire_format_bytes[i] as usize;
        if len == 0 { break; }
        let end = i + len + 1;
        let mut span = wire_format_bytes[i+1..end].to_vec();
        span.push('.' as u8);
        result.push(span);
        i = end;
    };

    let flat: Vec<_> = result.into_iter().flatten().collect();

    let name = String::from_utf8(flat).unwrap();

    // Remove the trailing '.' if it exists (it should always exist)
    if name.ends_with('.') {
        name[0..name.len()-1].to_string()
    } else {
        name
    }
}