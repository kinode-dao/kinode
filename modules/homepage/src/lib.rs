use serde_json::json;
use uqbar_process_lib::{get_payload, receive, Address, Message, Payload, Request, Response};

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;

const HOME_PAGE: &str = include_str!("home.html");

fn serialize_json_message(message: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(message)?)
}

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();
        println!("homepage: start");

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!("homepage: ended with error: {:?}", e);
            }
        }
    }
}

fn main(our: Address) -> anyhow::Result<()> {
    // bind to root path on http_server
    Request::new()
        .target(Address::new(&our.node, "http_server:sys:uqbar")?)?
        .ipc(
            &json!({
                "BindPath": {
                    "path": "/",
                    "authenticated": true,
                    "local_only": false
                }
            }),
            serialize_json_message,
        )?
        .send()?;

    loop {
        let Ok((_source, message)) = receive() else {
            println!("homepage: got network error");
            continue;
        };
        let Message::Request(request) = message else {
            println!("homepage: got unexpected message: {:?}", message);
            continue;
        };

        let message_json: serde_json::Value = match serde_json::from_slice(&request.ipc) {
            Ok(v) => v,
            Err(_) => {
                println!("homepage: failed to parse ipc JSON, skipping");
                continue;
            }
        };

        if message_json["path"] == "/" && message_json["method"] == "GET" {
            println!("homepage: sending response");
            Response::new()
                .ipc(
                    &json!({
                        "action": "response",
                        "status": 200,
                        "headers": {
                            "Content-Type": "text/html",
                        },
                    }),
                    serialize_json_message,
                )?
                .payload(Payload {
                    mime: Some("text/html".to_string()),
                    bytes: HOME_PAGE
                        .replace("${our}", &our.node)
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                })
                .send()?;
        } else if message_json["path"].is_string() {
            Response::new()
                .ipc(
                    &json!({
                        "action": "response",
                        "status": 404,
                        "headers": {
                            "Content-Type": "text/html",
                        },
                    }),
                    serialize_json_message,
                )?
                .payload(Payload {
                    mime: Some("text/html".to_string()),
                    bytes: "Not Found".to_string().as_bytes().to_vec(),
                })
                .send()?;
        } else if message_json["hello"] == "world" {
            Response::new()
                .ipc(
                    &json!({
                        "hello": "to you too"
                    }),
                    serialize_json_message,
                )?
                .payload(Payload {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::json!({
                        "hello": "to you too"
                    })
                    .to_string()
                    .as_bytes()
                    .to_vec(),
                })
                .send()?;
        } else {
            if let Some(payload) = get_payload() {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&payload.bytes) {
                    // println!("JSON: {}", json);
                    if json["message"] == "ping" {
                        // WebSocket pushes are sent as requests
                        Request::new()
                            .target(Address::new(&our.node, "encryptor:sys:uqbar")?)?
                            .ipc(
                                &json!({
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
                                        "json": Some(json!({ // this is the JSON to forward
                                            "WebSocketPush": {
                                                "target": {
                                                    "node": our.node.clone(),
                                                    "id": "homepage", // If the message passed in an ID then we could send to just that ID
                                                }
                                            }
                                        })),
                                    }

                                }),
                                serialize_json_message,
                            )?
                            .payload(Payload {
                                mime: Some("application/json".to_string()),
                                bytes: serde_json::json!({
                                    "pong": true
                                })
                                .to_string()
                                .as_bytes()
                                .to_vec(),
                            })
                            .send()?;
                    }
                }
            }
        }
    }
}
