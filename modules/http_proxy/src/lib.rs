use serde_json::json;
use std::collections::HashMap;
use uqbar_process_lib::{
    get_payload, grant_messaging, println, receive, Address, Message, Payload, ProcessId, Request,
    Response,
};

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;
impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();

        grant_messaging(
            &our,
            &Vec::from([ProcessId::from_str("http_server:sys:uqbar").unwrap()]),
        );

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!("http_proxy: ended with error: {:?}", e);
            }
        }
    }
}

const PROXY_HOME_PAGE: &str = include_str!("http_proxy.html");

fn serialize_json_message(message: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(message)?)
}

fn send_http_response(
    status: u16,
    headers: HashMap<String, String>,
    payload_bytes: Vec<u8>,
) -> anyhow::Result<()> {
    Response::new()
        .ipc(
            &json!({
                "status": status,
                "headers": headers,
            }),
            serialize_json_message,
        )?
        .payload(Payload {
            mime: Some("text/html".to_string()),
            bytes: payload_bytes,
        })
        .send()?;
    Ok(())
}

fn send_not_found() -> anyhow::Result<()> {
    send_http_response(
        404,
        HashMap::new(),
        "Not Found".to_string().as_bytes().to_vec(),
    )
}

fn main(our: Address) -> anyhow::Result<()> {
    let mut registrations: HashMap<String, String> = HashMap::new();

    // bind to all of our favorite paths
    for path in ["/", "/static/*", "/list", "/register", "/serve/:username/*"] {
        Request::new()
            .target(Address::new(&our.node, "http_server:sys:uqbar")?)?
            .ipc(
                &json!({
                    "BindPath": {
                        "path": path,
                        "authenticated": true,
                        "local_only": false
                    }
                }),
                serialize_json_message,
            )?
            .send()?;
    }

    loop {
        let Ok((_source, message)) = receive() else {
            //print_to_terminal(0, "http_proxy: got network error");
            let mut headers = HashMap::new();
            headers.insert("Content-Type".to_string(), "text/html".to_string());
            send_http_response(
                503,
                headers,
                format!("<h1>Node Offline</h1>").as_bytes().to_vec(),
            )?;
            continue;
        };
        let Message::Request(request) = message else {
            println!("http_proxy: got unexpected message");
            continue;
        };

        let message_json: serde_json::Value = match serde_json::from_slice(&request.ipc) {
            Ok(v) => v,
            Err(_) => {
                //print_to_terminal(1, "http_proxy: failed to parse ipc JSON, skipping");
                continue;
            }
        };

        //print_to_terminal(
        //    1,
        //    format!("http_proxy: got request: {}", message_json).as_str(),
        //);

        if message_json["path"] == "/" && message_json["method"] == "GET" {
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
                    bytes: PROXY_HOME_PAGE
                        .replace("${our}", &our.node)
                        .as_bytes()
                        .to_vec(),
                })
                .send()?;
        } else if message_json["path"] == "/list" && message_json["method"] == "GET" {
            Response::new()
                .ipc(
                    &json!({
                        "action": "response",
                        "status": 200,
                        "headers": {
                            "Content-Type": "application/json",
                        },
                    }),
                    serialize_json_message,
                )?
                .payload(Payload {
                    mime: Some("application/json".to_string()),
                    bytes: serde_json::json!({"registrations": registrations})
                        .to_string()
                        .as_bytes()
                        .to_vec(),
                })
                .send()?;
        } else if message_json["path"] == "/register" && message_json["method"] == "POST" {
            let mut status = 204;

            let Some(payload) = get_payload() else {
                //print_to_terminal(1, "/register POST with no bytes");
                continue;
            };

            let body: serde_json::Value = match serde_json::from_slice(&payload.bytes) {
                Ok(s) => s,
                Err(e) => {
                    //print_to_terminal(1, format!("Bad body format: {}", e).as_str());
                    continue;
                }
            };

            let username = body["username"].as_str().unwrap_or("");

            //print_to_terminal(1, format!("Register proxy for: {}", username).as_str());

            if !username.is_empty() {
                registrations.insert(username.to_string(), "foo".to_string());
            } else {
                status = 400;
            }

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
                    bytes: (if status == 400 {
                        "Bad Request"
                    } else {
                        "Success"
                    })
                    .to_string()
                    .as_bytes()
                    .to_vec(),
                })
                .send()?;
        } else if message_json["path"] == "/register" && message_json["method"] == "DELETE" {
            //print_to_terminal(1, "HERE IN /register to delete something");
            let username = message_json["query_params"]["username"]
                .as_str()
                .unwrap_or("");

            let mut status = 204;

            if !username.is_empty() {
                registrations.remove(username);
            } else {
                status = 400;
            }

            Response::new()
                .ipc(
                    &json!({
                        "action": "response",
                        "status": status,
                        "headers": {
                            "Content-Type": "text/html",
                        },
                    }),
                    serialize_json_message,
                )?
                .payload(Payload {
                    mime: Some("text/html".to_string()),
                    bytes: (if status == 400 {
                        "Bad Request"
                    } else {
                        "Success"
                    })
                    .to_string()
                    .as_bytes()
                    .to_vec(),
                })
                .send()?;
        } else if message_json["path"] == "/serve/:username/*" {
            let username = message_json["url_params"]["username"]
                .as_str()
                .unwrap_or("");
            let raw_path = message_json["raw_path"].as_str().unwrap_or("");
            //print_to_terminal(1, format!("proxy for user: {}", username).as_str());

            if username.is_empty() || raw_path.is_empty() {
                send_not_found()?;
            } else if !registrations.contains_key(username) {
                Response::new()
                    .ipc(
                        &json!({
                            "action": "response",
                            "status": 403,
                            "headers": {
                                "Content-Type": "text/html",
                            },
                        }),
                        serialize_json_message,
                    )?
                    .payload(Payload {
                        mime: Some("text/html".to_string()),
                        bytes: "Not Authorized".to_string().as_bytes().to_vec(),
                    })
                    .send()?;
            } else {
                let path_parts: Vec<&str> = raw_path.split('/').collect();
                let mut proxied_path = "/".to_string();

                if let Some(pos) = path_parts.iter().position(|&x| x == "serve") {
                    proxied_path = format!("/{}", path_parts[pos + 2..].join("/"));
                    //print_to_terminal(1, format!("Path to proxy: {}", proxied_path).as_str());
                }

                Request::new()
                    .target(Address::new(&username, "http_server:sys:uqbar")?)?
                    .inherit(true)
                    .ipc(
                        &json!({
                            "method": message_json["method"],
                            "path": proxied_path,
                            "headers": message_json["headers"],
                            "proxy_path": raw_path,
                            "query_params": message_json["query_params"],
                        }),
                        serialize_json_message,
                    )?
                    .send()?;
            }
        } else {
            send_not_found()?;
        }
    }
}
