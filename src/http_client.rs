use crate::types::*;
use anyhow::Result;
use http::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

// Test http_client with these commands in the terminal
// !message our http_client {"method": "GET", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {}}
// !message our http_client {"method": "POST", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}
// !message our http_client {"method": "PUT", "url": "https://jsonplaceholder.typicode.com/posts", "headers": {"Content-Type": "application/json"}}

//
// http_client.rs types
//

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,          // must parse to http::Method
    pub version: Option<String>, // must parse to http::Version
    pub url: String,             // must parse to url::Url
    pub headers: HashMap<String, String>,
    // BODY is stored in the payload, as bytes
    // TIMEOUT is stored in the message expect_response
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    // BODY is stored in the payload, as bytes
}

#[derive(Error, Debug, Serialize, Deserialize)]
pub enum HttpClientError {
    #[error("http_client: request could not be parsed to HttpRequest: {}.", req)]
    BadRequest { req: String },
    #[error("http_client: http method not supported: {}", method)]
    BadMethod { method: String },
    #[error("http_client: url could not be parsed: {}", url)]
    BadUrl { url: String },
    #[error("http_client: http version not supported: {}", version)]
    BadVersion { version: String },
    #[error("http_client: failed to execute request {}", error)]
    RequestFailed { error: String },
}

pub async fn http_client(
    our_name: String,
    send_to_loop: MessageSender,
    mut recv_in_client: MessageReceiver,
    _print_tx: PrintSender,
) -> Result<()> {
    let client = reqwest::Client::new();
    let our_name = Arc::new(our_name);

    while let Some(KernelMessage {
        id,
        source,
        rsvp,
        message:
            Message::Request(Request {
                expects_response,
                ipc,
                ..
            }),
        payload,
        ..
    }) = recv_in_client.recv().await
    {
        tokio::spawn(handle_message(
            our_name.clone(),
            id,
            rsvp.unwrap_or(source),
            expects_response,
            ipc,
            payload,
            client.clone(),
            send_to_loop.clone(),
        ));
    }
    Err(anyhow::anyhow!("http_client: loop died"))
}

async fn handle_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    json: Vec<u8>,
    body: Option<Payload>,
    client: reqwest::Client,
    send_to_loop: MessageSender,
) {
    let req: HttpRequest = match serde_json::from_slice(&json) {
        Ok(req) => req,
        Err(_e) => {
            make_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::BadRequest {
                    req: String::from_utf8(json).unwrap_or_default(),
                },
                send_to_loop,
            )
            .await;
            return;
        }
    };

    let Ok(req_method) = http::Method::from_bytes(req.method.as_bytes()) else {
        make_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::BadMethod { method: req.method },
            send_to_loop,
        )
        .await;
        return;
    };

    let mut request_builder = client.request(req_method, req.url);

    if let Some(version) = req.version {
        request_builder = match version.as_str() {
            "HTTP/0.9" => request_builder.version(http::Version::HTTP_09),
            "HTTP/1.0" => request_builder.version(http::Version::HTTP_10),
            "HTTP/1.1" => request_builder.version(http::Version::HTTP_11),
            "HTTP/2.0" => request_builder.version(http::Version::HTTP_2),
            "HTTP/3.0" => request_builder.version(http::Version::HTTP_3),
            _ => {
                make_error_message(
                    our,
                    id,
                    target,
                    expects_response,
                    HttpClientError::BadVersion { version },
                    send_to_loop,
                )
                .await;
                return;
            }
        }
    }

    if let Some(payload) = body {
        request_builder = request_builder.body(payload.bytes);
    }

    let Ok(request) = request_builder
        .headers(deserialize_headers(req.headers))
        .build()
    else {
        make_error_message(
            our,
            id,
            target,
            expects_response,
            HttpClientError::RequestFailed {
                error: "failed to build request".into(),
            },
            send_to_loop,
        )
        .await;
        return;
    };

    match client.execute(request).await {
        Ok(response) => {
            if expects_response.is_some() {
                let _ = send_to_loop
                    .send(KernelMessage {
                        id,
                        source: Address {
                            node: our.to_string(),
                            process: ProcessId::new(Some("http_client"), "sys", "uqbar"),
                        },
                        target,
                        rsvp: None,
                        message: Message::Response((
                            Response {
                                inherit: false,
                                ipc: serde_json::to_vec::<Result<HttpResponse, HttpClientError>>(
                                    &Ok(HttpResponse {
                                        status: response.status().as_u16(),
                                        headers: serialize_headers(&response.headers()),
                                    }),
                                )
                                .unwrap(),
                                metadata: None,
                            },
                            None,
                        )),
                        payload: Some(Payload {
                            mime: None,
                            bytes: response.bytes().await.unwrap_or_default().to_vec(),
                        }),
                        signed_capabilities: None,
                    })
                    .await;
            }
        }
        Err(e) => {
            make_error_message(
                our,
                id,
                target,
                expects_response,
                HttpClientError::RequestFailed {
                    error: e.to_string(),
                },
                send_to_loop,
            )
            .await;
        }
    }
}

//
//  helpers
//

fn to_pascal_case(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<String>>()
        .join("-")
}

fn serialize_headers(headers: &HeaderMap) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();
    for (key, value) in headers.iter() {
        let key_str = to_pascal_case(key.as_ref());
        let value_str = value.to_str().unwrap_or("").to_string();
        hashmap.insert(key_str, value_str);
    }
    hashmap
}

fn deserialize_headers(hashmap: HashMap<String, String>) -> HeaderMap {
    let mut header_map = HeaderMap::new();
    for (key, value) in hashmap {
        let key_bytes = key.as_bytes();
        let key_name = HeaderName::from_bytes(key_bytes).unwrap();
        let value_header = HeaderValue::from_str(&value).unwrap();
        header_map.insert(key_name, value_header);
    }
    header_map
}

async fn make_error_message(
    our: Arc<String>,
    id: u64,
    target: Address,
    expects_response: Option<u64>,
    error: HttpClientError,
    send_to_loop: MessageSender,
) {
    if expects_response.is_some() {
        let _ = send_to_loop
            .send(KernelMessage {
                id,
                source: Address {
                    node: our.to_string(),
                    process: ProcessId::new(Some("http_client"), "sys", "uqbar"),
                },
                target,
                rsvp: None,
                message: Message::Response((
                    Response {
                        inherit: false,
                        ipc: serde_json::to_vec::<Result<HttpResponse, HttpClientError>>(&Err(
                            error,
                        ))
                        .unwrap(),
                        metadata: None,
                    },
                    None,
                )),
                payload: None,
                signed_capabilities: None,
            })
            .await;
    }
}
