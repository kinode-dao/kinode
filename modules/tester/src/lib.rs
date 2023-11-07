use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

use uqbar_process_lib::{Address, ProcessId, Request, Response};
use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
enum TesterRequest {
    Run,
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
enum TesterError {
    #[error("RejectForeign")]
    RejectForeign,
    #[error("UnexpectedResponse")]
    UnexpectedResponse,
}

fn make_vfs_address(our: &wit::Address) -> anyhow::Result<Address> {
    Ok(wit::Address {
        node: our.node.clone(),
        process: ProcessId::from_str("vfs:sys:uqbar")?,
    })
}

fn handle_message (our: &Address) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(TesterError::RejectForeign.into());
    }

    match message {
        wit::Message::Response(_) => {
            return Err(TesterError::UnexpectedResponse.into());
        },
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
                TesterRequest::Run => {
                    wit::print_to_terminal(0, "tester: got Run");

                    let (_, response) = Request::new()
                        .target(make_vfs_address(&our)?)?
                        .ipc_bytes(serde_json::to_vec(&kt::VfsRequest {
                            drive: "tester:uqbar".into(),
                            action: kt::VfsAction::GetEntry("/".into()),
                        })?)
                        .send_and_await_response(5)??;

                    let wit::Message::Response((response, _)) = response else { panic!("") };
                    let kt::VfsResponse::GetEntry { children, .. } =
                        serde_json::from_slice(&ipc)? else { panic!("") };
                    let mut children: HashSet<_> = children.into_iter().collect();
                    children.remove("/manifest.json");
                    children.remove("/metadata.json");
                    children.remove("/tester.wasm");

                    wit::print_to_terminal(0, &format!("tester: running {:?}...", children));

                    for child in &children {
                        let (_, response) = Request::new()
                            .target(make_vfs_address(&our)?)?
                            .ipc_bytes(serde_json::to_vec(&kt::VfsRequest {
                                drive: "tester:uqbar".into(),
                                action: kt::VfsAction::GetEntryLength(child.into()),
                            })?)
                            .send_and_await_response(5)??;

                        let wit::Message::Response((response, _)) = response else { panic!("") };
                        let kt::VfsResponse::GetEntryLength(length) =
                            serde_json::from_slice(&ipc)? else { panic!("") };

                        wit::print_to_terminal(0, &format!("tester: child {} length {:?}", child, length));

                        match wit::spawn(
                            None,
                            child,
                            &wit::OnPanic::None, //  TODO: notify us
                            &wit::Capabilities::All,
                            false, // not public
                        ) {
                            Ok(child_process_id) => child_process_id,
                            Err(e) => {
                                wit::print_to_terminal(0, &format!("couldn't spawn {}: {}", child, e));
                                panic!("couldn't spawn"); //  TODO
                            }
                        };
                    }
                    Response::new()
                        .ipc_bytes(serde_json::to_vec(&ipc).unwrap())
                        .send()
                        .unwrap();
                },
            }
            Ok(())
        },
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(0, "tester: begin");

        let our = Address::from_str(&our).unwrap();

        // orchestrate tests using external scripts
        //  -> must give drive cap to rpc
        // TODO: need read as well?
        let drive_cap = wit::get_capability(
            &make_vfs_address(&our).unwrap(),
            &serde_json::to_string(&serde_json::json!({
                "kind": "write",
                "drive": "tester:uqbar",
            })).unwrap()
        ).unwrap();
        wit::share_capability(&ProcessId::from_str("http_server:sys:uqbar").unwrap(), &drive_cap);

        loop {
            match handle_message(&our) {
                Ok(()) => {},
                Err(e) => {
                    wit::print_to_terminal(0, format!(
                        "tester: error: {:?}",
                        e,
                    ).as_str());
                    // if let Some(e) = e.downcast_ref::<sq::SqliteError>() {
                    //     send_response(
                    //         &Response {
                    //             inherit: false,
                    //             ipc: serde_json::to_vec(&e).unwrap(),
                    //             metadata: None,
                    //         },
                    //         None,
                    //     );
                    // }
                },
            };
        }
    }
}
