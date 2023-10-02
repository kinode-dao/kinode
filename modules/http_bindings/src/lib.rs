cargo_component_bindings::generate!();

use hmac::{Hmac, Mac};
use jwt::{Error, SignWithKey, VerifyWithKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use url::form_urlencoded;

use bindings::component::uq_process::types::*;
use bindings::{get_payload, print_to_terminal, receive, send_request, send_response, Guest};

mod process_lib;

struct Component;

#[derive(Debug, Serialize, Deserialize)]
struct BoundPath {
    app: String,
    authenticated: bool,
    local_only: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    username: String,
    expiration: u64,
}

fn generate_token(our_node: String, secret: Hmac<Sha256>) -> Option<String> {
    let claims = JwtClaims {
        username: our_node,
        expiration: 0,
    };
    let token: Option<String> = match claims.sign_with_key(&secret) {
        Ok(token) => Some(token),
        Err(_) => None,
    };
    token
}

fn auth_cookie_valid(our_node: String, cookie: &str, secret: Hmac<Sha256>) -> bool {
    let cookie_parts: Vec<&str> = cookie.split("; ").collect();
    let mut auth_token = None;
    for cookie_part in cookie_parts {
        let cookie_part_parts: Vec<&str> = cookie_part.split("=").collect();
        if cookie_part_parts.len() == 2
            && cookie_part_parts[0] == format!("uqbar-auth_{}", our_node)
        {
            auth_token = Some(cookie_part_parts[1].to_string());
            break;
        }
    }

    let auth_token = match auth_token {
        Some(token) if !token.is_empty() => token,
        _ => return false,
    };

    print_to_terminal(
        1,
        format!("http_bindings: auth_token: {}", auth_token).as_str(),
    );

    let claims: Result<JwtClaims, Error> = auth_token.verify_with_key(&secret);

    match claims {
        Ok(data) => {
            print_to_terminal(
                1,
                format!(
                    "http_bindings: our name: {}, token_name {}",
                    our_node, data.username
                )
                .as_str(),
            );
            data.username == our_node
        }
        Err(_) => {
            print_to_terminal(1, "http_bindings: failed to verify token");
            false
        }
    }
}

fn send_http_response(status: u16, headers: HashMap<String, String>, payload_bytes: Vec<u8>) {
    send_response(
        &Response {
            ipc: Some(
                serde_json::json!({
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

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(1, "http_bindings: start");
        let mut path_bindings: HashMap<String, BoundPath> = HashMap::new();
        let mut jwt_secret: Option<Hmac<Sha256>> = None;

        // get jwt secret from http_server, handle as a request with set-jwt-secret
        send_request(
            &Address {
                node: our.node.clone(),
                process: ProcessId::Name("http_server".to_string()),
            },
            &Request {
                inherit: false,
                expects_response: None,
                ipc: Some(
                    serde_json::json!({
                        "ServerAction": {
                            "action": "get-jwt-secret",
                        }
                    })
                    .to_string(),
                ),
                metadata: None,
            },
            None,
            None,
        );

        loop {
            let Ok((source, message)) = receive() else {
                print_to_terminal(0, "http_bindings: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                // Ignore responses for now
                print_to_terminal(0, "http_bindings: got unexpected Respose, ignoring");
                continue;
            };

            let Some(json) = request.ipc else {
                print_to_terminal(0, "http_bindings: no ipc JSON, skipping");
                continue;
            };

            let message_json: serde_json::Value = match serde_json::from_str(&json) {
                Ok(v) => v,
                Err(_) => {
                    print_to_terminal(1, "http_bindings: failed to parse ipc JSON, skipping");
                    continue;
                }
            };

            let action = message_json["action"].as_str().unwrap_or("");
            let address = message_json["address"].as_str().unwrap_or(""); // origin HTTP address
            let path = message_json["path"].as_str().unwrap_or("");
            let app = match source.process {
                ProcessId::Name(name) => name,
                _ => "".to_string(),
            };

            print_to_terminal(1, "http_bindings: got message");

            if action == "set-jwt-secret" {
                let Some(payload) = get_payload() else {
                    panic!("set-jwt-secret with no payload");
                };

                let jwt_secret_bytes = payload.bytes;

                print_to_terminal(1, "http_bindings: generating token secret...");
                jwt_secret = match Hmac::new_from_slice(&jwt_secret_bytes) {
                    Ok(secret) => Some(secret),
                    Err(_) => {
                        print_to_terminal(1, "http_bindings: failed to generate token secret");
                        None
                    }
                };
                send_response(
                    &Response {
                        ipc: None,
                        metadata: None,
                    },
                    None,
                );
            } else if action == "bind-app" && path != "" && app != "" {
                print_to_terminal(1, "http_bindings: binding app 1");
                let path_segments = path
                    .trim_start_matches('/')
                    .split("/")
                    .collect::<Vec<&str>>();
                if app != "apps_home"
                    && (path_segments.is_empty()
                        || path_segments[0] != app.clone().replace("_", "-"))
                {
                    print_to_terminal(
                        1,
                        format!(
                            "http_bindings: first path segment does not match process: {}",
                            path
                        )
                        .as_str(),
                    );
                    continue;
                } else {
                    print_to_terminal(
                        1,
                        format!("http_bindings: binding app 2 {}", path.to_string()).as_str(),
                    );
                    path_bindings.insert(path.to_string(), {
                        BoundPath {
                            app: app.to_string(),
                            authenticated: message_json
                                .get("authenticated")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                            local_only: message_json
                                .get("local_only")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        }
                    });
                }
            } else if action == "request" {
                print_to_terminal(1, "http_bindings: got request");

                // Start Login logic
                if path == "/login" {
                    print_to_terminal(1, "http_bindings: got login request");

                    if message_json["method"] == "GET" {
                        print_to_terminal(1, "http_bindings: got login GET request");
                        let login_page_content = include_str!("login.html");
                        let personalized_login_page =
                            login_page_content.replace("${our}", &our.node);

                        send_http_response(
                            200,
                            {
                                let mut headers = HashMap::new();
                                headers.insert("Content-Type".to_string(), "text/html".to_string());
                                headers
                            },
                            personalized_login_page.as_bytes().to_vec(),
                        );
                    } else if message_json["method"] == "POST" {
                        print_to_terminal(1, "http_bindings: got login POST request");

                        let Some(payload) = get_payload() else {
                            panic!("/login POST with no bytes");
                        };
                        let body_json_string = match String::from_utf8(payload.bytes) {
                            Ok(s) => s,
                            Err(_) => String::new(),
                        };
                        let body: serde_json::Value =
                            serde_json::from_str(&body_json_string).unwrap();
                        let password = body["password"].as_str().unwrap_or("");

                        if password == "" {
                            send_http_response(
                                400,
                                HashMap::new(),
                                "Bad Request".as_bytes().to_vec(),
                            );
                        }

                        match jwt_secret.clone() {
                            Some(secret) => {
                                match generate_token(our.node.clone(), secret) {
                                    Some(token) => {
                                        // Token was generated successfully; you can use it here.
                                        send_http_response(
                                            200,
                                            {
                                                let mut headers = HashMap::new();
                                                headers.insert(
                                                    "Content-Type".to_string(),
                                                    "text/html".to_string(),
                                                );
                                                headers.insert(
                                                    "set-cookie".to_string(),
                                                    format!("uqbar-auth_{}={};", our.node, token),
                                                );
                                                headers
                                            },
                                            "".as_bytes().to_vec(),
                                        );
                                    }
                                    None => {
                                        print_to_terminal(1, "so secret 1");
                                        // Failed to generate token; you should probably return an error.
                                        send_http_response(
                                            500,
                                            HashMap::new(),
                                            "Server Error".as_bytes().to_vec(),
                                        );
                                    }
                                }
                            }
                            None => {
                                print_to_terminal(1, "so secret 2");
                                send_http_response(
                                    500,
                                    HashMap::new(),
                                    "Server Error".as_bytes().to_vec(),
                                );
                            }
                        }
                    } else if message_json["method"] == "PUT" {
                        print_to_terminal(1, "http_bindings: got login PUT request");

                        let Some(payload) = get_payload() else {
                            panic!("/login PUT with no bytes");
                        };
                        let body_json_string = match String::from_utf8(payload.bytes) {
                            Ok(s) => s,
                            Err(_) => String::new(),
                        };
                        let body: serde_json::Value =
                            serde_json::from_str(&body_json_string).unwrap();
                        // let password = body["password"].as_str().unwrap_or("");
                        let signature = body["signature"].as_str().unwrap_or("");

                        if signature == "" {
                            send_http_response(
                                400,
                                HashMap::new(),
                                "Bad Request".as_bytes().to_vec(),
                            );
                        } else {
                            // TODO: Check signature against our address
                            print_to_terminal(1, "http_bindings: generating secret...");
                            // jwt_secret = generate_secret(password);
                            print_to_terminal(1, "http_bindings: generating token...");

                            match jwt_secret.clone() {
                                Some(secret) => {
                                    match generate_token(our.node.clone(), secret) {
                                        Some(token) => {
                                            // Token was generated successfully; you can use it here.
                                            send_http_response(
                                                200,
                                                {
                                                    let mut headers = HashMap::new();
                                                    headers.insert(
                                                        "Content-Type".to_string(),
                                                        "text/html".to_string(),
                                                    );
                                                    headers.insert(
                                                        "set-cookie".to_string(),
                                                        format!(
                                                            "uqbar-auth_{}={};",
                                                            our.node, token
                                                        ),
                                                    );
                                                    headers
                                                },
                                                "".as_bytes().to_vec(),
                                            );
                                        }
                                        None => {
                                            // Failed to generate token; you should probably return an error.
                                            send_http_response(
                                                500,
                                                HashMap::new(),
                                                "Server Error".as_bytes().to_vec(),
                                            );
                                        }
                                    }
                                }
                                None => {
                                    send_http_response(
                                        500,
                                        HashMap::new(),
                                        "Server Error".as_bytes().to_vec(),
                                    );
                                }
                            }
                        }
                    } else {
                        send_http_response(404, HashMap::new(), "Not Found".as_bytes().to_vec());
                    }
                    continue;
                }
                // End Login logic

                // Start Encryption Secret Logic
                if path == "/encryptor" {
                    bindings::print_to_terminal(1, "http_bindings: got encryptor request");
                    let auth_success = match jwt_secret.clone() {
                        Some(secret) => {
                            bindings::print_to_terminal(1, "HAVE SECRET");
                            auth_cookie_valid(
                                our.node.clone(),
                                message_json["headers"]["cookie"].as_str().unwrap_or(""),
                                secret,
                            )
                        }
                        None => {
                            bindings::print_to_terminal(1, "NO SECRET");
                            false
                        }
                    };

                    if auth_success {
                        let body_bytes = match get_payload() {
                            Some(payload) => payload.bytes,
                            None => vec![],
                        };
                        let body_json_string = match String::from_utf8(body_bytes) {
                            Ok(s) => s,
                            Err(_) => String::new(),
                        };
                        let body: serde_json::Value =
                            serde_json::from_str(&body_json_string).unwrap();
                        let channel_id = body["channel_id"].as_str().unwrap_or("");
                        let public_key_hex = body["public_key_hex"].as_str().unwrap_or("");

                        send_request(
                            &Address {
                                node: our.node.clone(),
                                process: ProcessId::Name("encryptor".to_string()),
                            },
                            &Request {
                                inherit: true,
                                expects_response: None,
                                ipc: Some(
                                    serde_json::json!({
                                        "GetKeyAction": {
                                            "channel_id": channel_id,
                                            "public_key_hex": public_key_hex,
                                        }
                                    })
                                    .to_string(),
                                ),
                                metadata: None,
                            },
                            None,
                            None,
                        );
                    } else {
                        send_http_response(401, HashMap::new(), "Unauthorized".as_bytes().to_vec());
                    }
                    continue;
                }
                // End Encryption Secret Logic

                let path_segments = path
                    .trim_start_matches('/')
                    .trim_end_matches('/')
                    .split("/")
                    .collect::<Vec<&str>>();
                let mut registered_path = path;
                let mut url_params: HashMap<String, String> = HashMap::new();

                for (key, _value) in &path_bindings {
                    let key_segments = key
                        .trim_start_matches('/')
                        .trim_end_matches('/')
                        .split("/")
                        .collect::<Vec<&str>>();
                    if key_segments.len() != path_segments.len()
                        && (!key.contains("/.*") || (key_segments.len() - 1) > path_segments.len())
                    {
                        continue;
                    }

                    let mut paths_match = true;
                    for i in 0..key_segments.len() {
                        if key_segments[i] == "*" {
                            break;
                        } else if !(key_segments[i].starts_with(":")
                            || key_segments[i] == path_segments[i])
                        {
                            paths_match = false;
                            break;
                        } else if key_segments[i].starts_with(":") {
                            url_params.insert(
                                key_segments[i][1..].to_string(),
                                path_segments[i].to_string(),
                            );
                        }
                    }

                    if paths_match {
                        registered_path = key;
                        break;
                    }
                    url_params = HashMap::new();
                }

                print_to_terminal(
                    1,
                    &("http_bindings: registered path ".to_string() + registered_path),
                );

                match path_bindings.get(registered_path) {
                    Some(bound_path) => {
                        let app = bound_path.app.as_str();
                        print_to_terminal(
                            1,
                            &("http_bindings: properly unwrapped path ".to_string()
                                + registered_path),
                        );

                        if bound_path.authenticated {
                            print_to_terminal(1, "AUTHENTICATED ROUTE");
                            let auth_success = match jwt_secret.clone() {
                                Some(secret) => auth_cookie_valid(
                                    our.node.clone(),
                                    message_json["headers"]["cookie"].as_str().unwrap_or(""),
                                    secret,
                                ),
                                None => {
                                    print_to_terminal(1, "NO SECRET");
                                    false
                                }
                            };

                            if !auth_success {
                                print_to_terminal(1, "http_bindings: failure to authenticate");
                                let proxy_path = message_json["proxy_path"].as_str();

                                let redirect_path: String = match proxy_path {
                                    Some(pp) => {
                                        form_urlencoded::byte_serialize(pp.as_bytes()).collect()
                                    }
                                    None => {
                                        form_urlencoded::byte_serialize(path.as_bytes()).collect()
                                    }
                                };

                                let location = match proxy_path {
                                    Some(_) => format!(
                                        "/http-proxy/serve/{}/login?redirect={}",
                                        &our.node, redirect_path
                                    ),
                                    None => format!("/login?redirect={}", redirect_path),
                                };

                                send_http_response(
                                    302,
                                    {
                                        let mut headers = HashMap::new();
                                        headers.insert(
                                            "Content-Type".to_string(),
                                            "text/html".to_string(),
                                        );
                                        headers.insert("Location".to_string(), location);
                                        headers
                                    },
                                    "Auth cookie not valid".as_bytes().to_vec(),
                                );
                                continue;
                            }
                        }

                        if bound_path.local_only && !address.starts_with("127.0.0.1:") {
                            send_http_response(
                                403,
                                {
                                    let mut headers = HashMap::new();
                                    headers.insert(
                                        "Content-Type".to_string(),
                                        "text/html".to_string(),
                                    );
                                    headers
                                },
                                "<h1>Localhost Origin Required</h1>".as_bytes().to_vec(),
                            );
                            continue;
                        }

                        // import send-request: func(target: address, request: request, context: option<context>, payload: option<payload>)
                        send_request(
                            &Address {
                                node: our.node.clone(),
                                process: ProcessId::Name(app.to_string()),
                            },
                            &Request {
                                inherit: true,
                                expects_response: None,
                                ipc: Some(
                                    serde_json::json!({
                                        "path": registered_path,
                                        "raw_path": path,
                                        "method": message_json["method"],
                                        "headers": message_json["headers"],
                                        "query_params": message_json["query_params"],
                                        "url_params": url_params,
                                    })
                                    .to_string(),
                                ),
                                metadata: None,
                            },
                            None,
                            get_payload().as_ref(),
                        );
                        continue;
                    }
                    None => {
                        print_to_terminal(1, "http_bindings: no app found at this path");
                        send_http_response(404, HashMap::new(), "Not Found".as_bytes().to_vec());
                    }
                }
            } else {
                print_to_terminal(
                    1,
                    format!(
                        "http_bindings: unexpected action: {:?}",
                        &message_json["action"],
                    )
                    .as_str(),
                );
            }
        }
    }
}

// TODO: handle auth correctly, generate a secret and store in filesystem if non-existent
