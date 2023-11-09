use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

use indexmap::map::IndexMap;

use uqbar_process_lib::{Address, ProcessId, Request, Response};
use uqbar_process_lib::kernel_types as kt;
use uqbar_process_lib::uqbar::process::standard as wit;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

type Rsvp = Option<kt::Address>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KernelMessage {
    pub id: u64,
    pub source: kt::Address,
    pub target: kt::Address,
    pub rsvp: Rsvp,
    pub message: kt::Message,
    pub payload: Option<kt::Payload>,
    pub signed_capabilities: Option<Vec<kt::SignedCapability>>,
}

#[derive(Debug, Serialize, Deserialize)]
enum TesterRequest {
    Run,
    KernelMessage(KernelMessage),
    GetFullMessage(kt::Message),
}

#[derive(Debug, Serialize, Deserialize)]
enum TesterResponse {
    Pass,
    Fail,
    GetFullMessage(Option<KernelMessage>),
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
enum TesterError {
    #[error("RejectForeign")]
    RejectForeign,
    #[error("UnexpectedResponse")]
    UnexpectedResponse,
}

type Messages = IndexMap<kt::Message, KernelMessage>;

fn make_vfs_address(our: &wit::Address) -> anyhow::Result<Address> {
    Ok(wit::Address {
        node: our.node.clone(),
        process: ProcessId::from_str("vfs:sys:uqbar")?,
    })
}

fn handle_message(our: &Address, messages: &mut Messages) -> anyhow::Result<()> {
    let (source, message) = wit::receive().unwrap();

    if our.node != source.node {
        return Err(TesterError::RejectForeign.into());
    }

    match message {
        wit::Message::Response((wit::Response { ipc, .. }, _)) => {
            match serde_json::from_slice(&ipc)? {
                TesterResponse::Pass | TesterResponse::Fail => {
                    if (source.process.package_name != "tester")
                       | (source.process.publisher_node != "uqbar") {
                        return Err(TesterError::UnexpectedResponse.into());
                    }
                    Response::new()
                        .ipc_bytes(ipc)
                        .send()
                        .unwrap();
                },
                TesterResponse::GetFullMessage(_) => { unimplemented!() }
            }
            Ok(())
        },
        wit::Message::Request(wit::Request { ipc, .. }) => {
            match serde_json::from_slice(&ipc)? {
                TesterRequest::Run => {
                    wit::print_to_terminal(0, "tester: got Run");

                    let child = "/test_runner.wasm";
                    let child_process_id = match wit::spawn(
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

                    Request::new()
                        .target(Address {
                            node: our.node.clone(),
                            process: child_process_id,
                        })?
                        .ipc_bytes(ipc.clone())
                        .expects_response(15)
                        .send()?;
                },
                TesterRequest::KernelMessage(kernel_message) => {
                    messages.insert(kernel_message.message.clone(), kernel_message);
                },
                TesterRequest::GetFullMessage(message) => {
                    Response::new()
                        .ipc_bytes(serde_json::to_vec(&TesterResponse::GetFullMessage(
                            match messages.get(&message) {
                                None => None,
                                Some(m) => Some(m.clone()),
                            }
                        )).unwrap())
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
        let mut messages: Messages = IndexMap::new();

        // orchestrate tests using external scripts
        //  -> must give drive cap to rpc
        let drive_cap = wit::get_capability(
            &make_vfs_address(&our).unwrap(),
            &serde_json::to_string(&serde_json::json!({
                "kind": "write",
                "drive": "tester:uqbar",
            })).unwrap()
        ).unwrap();
        wit::share_capability(&ProcessId::from_str("http_server:sys:uqbar").unwrap(), &drive_cap);

        loop {
            match handle_message(&our, &mut messages) {
                Ok(()) => {},
                Err(e) => {
                    wit::print_to_terminal(0, format!(
                        "tester: error: {:?}",
                        e,
                    ).as_str());
                    Response::new()
                        .ipc_bytes(serde_json::to_vec(&TesterResponse::Fail).unwrap())
                        .send()
                        .unwrap();
                },
            };
        }
    }
}
