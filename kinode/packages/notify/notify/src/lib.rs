#![feature(let_chains)]
use std::collections::HashMap;

use crate::kinode::process::notify::{
    Notification as Notif, Request as NotifyRequest, Response as NotifyResponse,
};
use kinode_process_lib::{
    await_message, call_init, get_blob, get_typed_state,
    homepage::add_to_homepage,
    http::{
        bind_http_path, send_response, HttpClientAction, HttpServerRequest, Method,
        OutgoingHttpRequest, StatusCode,
    },
    println, set_state, Address, LazyLoadBlob, ProcessId, Request, Response,
};
use serde::{Deserialize, Serialize};

wit_bindgen::generate!({
    path: "target/wit",
    world: "notify-sys-v0",
    generate_unused_types: true,
    additional_derives: [serde::Deserialize, serde::Serialize],
});

#[derive(Serialize, Deserialize)]
struct ProcessNotifConfig {
    allow: bool,
}

#[derive(Serialize, Deserialize)]
struct NotifState {
    config: HashMap<ProcessId, ProcessNotifConfig>,
    archive: HashMap<String, Vec<Notif>>,
    push_tokens: Vec<String>,
}

fn handle_message(our: &Address, state: &mut NotifState) -> anyhow::Result<()> {
    let message = await_message()?;

    if message.is_request() {
        let body = message.body();
        let source = message.source();
        match serde_json::from_slice(body)? {
            NotifyRequest::Push(ref notif) => {
                if source.node == our.node {
                    println!("push request: {}", source.process.clone().to_string());

                    state
                        .archive
                        .entry(source.process.clone().to_string())
                        .and_modify(|e| e.push(notif.clone()))
                        .or_insert(vec![notif.clone()]);

                    set_state(&bincode::serialize(&state)?);

                    if let Some(config) = state.config.get(&source.process)
                        && config.allow
                    {
                        // TODO: send notification
                    }
                } else {
                    // TODO: ignore notifications from other nodes?
                }
                Response::new()
                    .body(serde_json::to_vec(&NotifyResponse::Push).unwrap())
                    .send()
                    .unwrap();
            }
            NotifyRequest::History(ref process) => {
                println!("history request for process: {}", process);
                Response::new()
                    .body(
                        serde_json::to_vec(&NotifyResponse::History(
                            state
                                .archive
                                .get(process)
                                .map(|ns| ns.clone())
                                .unwrap_or_default(),
                        ))
                        .unwrap(),
                    )
                    .send()
                    .unwrap();
            }
        }
    } else if let Ok(req) = serde_json::from_slice::<HttpServerRequest>(message.body()) {
        match req {
            HttpServerRequest::Http(incoming) => {
                let path = incoming.bound_path(None);
                match path {
                    "/add-token" => {
                        if let Ok(Method::POST) = incoming.method()
                            && let Some(body) = get_blob()
                        {
                            let token: String = serde_json::from_slice(&body.bytes).unwrap();
                            state.push_tokens.push(token);
                            set_state(&bincode::serialize(&state)?);
                            send_response(StatusCode::CREATED, Some(HashMap::new()), vec![]);
                        } else {
                            send_response(StatusCode::BAD_REQUEST, Some(HashMap::new()), vec![]);
                        }
                    }
                    _ => {
                        send_response(
                            StatusCode::OK,
                            Some(HashMap::from([(
                                "Content-Type".to_string(),
                                "text/plain".to_string(),
                            )])),
                            "yes hello".as_bytes().to_vec(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    bind_http_path("/add-token", false, false).expect("failed to bind /add-token");

    let mut state: NotifState = match get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)) {
        Some(s) => s,
        None => NotifState {
            config: HashMap::new(),
            archive: HashMap::new(),
            push_tokens: vec![],
        },
    };

    add_to_homepage(
        "Notifications",
        None,
        None,
        Some(create_widget(&state.archive).as_str()),
    );

    loop {
        match handle_message(&our, &mut state) {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}

fn send_notif_to_expo(notif: &mut Notif) -> anyhow::Result<()> {
    let outgoing_request = OutgoingHttpRequest {
        method: "POST".to_string(),
        version: None,
        url: "https://exp.host/--/api/v2/push/send".to_string(),
        headers: HashMap::from_iter(vec![(
            "Content-Type".to_string(),
            "application/json".to_string(),
        )]),
    };
    let body = serde_json::to_vec(&HttpClientAction::Http(outgoing_request))?;

    if let Some(state) = get_typed_state(|bytes| Ok(bincode::deserialize::<NotifState>(bytes)?)) {
        notif.to = state.push_tokens.clone();
    }

    Request::new()
        .target(Address::new(
            "our",
            ProcessId::new(Some("http_client"), "distro", "sys"),
        ))
        .body(body)
        .expects_response(30)
        .blob(LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: serde_json::to_vec(notif)?,
        })
        .send()?;

    Ok(())
}

fn create_widget(notifs: &HashMap<String, Vec<Notif>>) -> String {
    let mut notifs_templated = String::new();
    for (_process, notifs) in notifs.iter() {
        for notif in notifs.iter() {
            notifs_templated.push_str(&format!(
                r#"<div class="notif">
                    <div class="title">{}</div>
                    <div class="body">{}</div>
                </div>"#,
                notif.title.clone().unwrap_or("Title".to_string()),
                notif.body.clone().unwrap_or("Body".to_string())
            ));
        }
    }
    format!(
        r#"<html>
        <head>
        <title>Notifications</title>
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>
            * {{
                margin: 0;
                padding: 0;
                box-sizing: border-box;
            }}
    
            body {{
                font-family: sans-serif;
                border-radius: 16px;
                backdrop-filter: saturate(1.25);
            }}
    
            .notifs {{
                display: flex;
                flex-direction: column;
                gap: 10px;
            }}
    
            .notif {{
                border: 1px solid #ccc;
                border-radius: 5px;
                padding: 10px;
                background: rgba(255, 255, 255, 0.1);
            }}
    
            .title {{
                font-weight: bold;
            }}
    
            .body {{
                font-size: 14px;
            }}
        </style>
        </head>
            <body>
                <div class="notifs">
                    {}
                </div>
            </body>
        </html>"#,
        notifs_templated,
    )
}
