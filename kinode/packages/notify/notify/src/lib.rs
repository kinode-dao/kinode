#![feature(let_chains)]
use std::collections::HashMap;

use crate::kinode::process::notify::{
    Notification as Notif, Request as NotifyRequest, Response as NotifyResponse,
};
use kinode_process_lib::{
    await_message, call_init, get_typed_state,
    http::{HttpClientAction, OutgoingHttpRequest},
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
}

fn handle_message(our: &Address, state: &mut NotifState) -> anyhow::Result<()> {
    let message = await_message()?;

    if !message.is_request() {
        // don't care
        return Ok(());
    }

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
    Ok(())
}

call_init!(init);
fn init(our: Address) {
    println!("begin");

    let mut state: NotifState = match get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)) {
        Some(s) => s,
        None => NotifState {
            config: HashMap::new(),
            archive: HashMap::new(),
        },
    };

    loop {
        match handle_message(&our, &mut state) {
            Ok(()) => {}
            Err(e) => {
                println!("error: {:?}", e);
            }
        };
    }
}

fn send_notif_to_expo(notif: &Notif) -> anyhow::Result<()> {
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
