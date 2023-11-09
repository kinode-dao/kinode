use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};

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

pub type Rsvp = Option<kt::Address>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelMessage {
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
                    wit::print_to_terminal(0, "test_runner: got Run");

                    let (_, response) = Request::new()
                        .target(make_vfs_address(&our)?)?
                        .ipc_bytes(serde_json::to_vec(&kt::VfsRequest {
                            drive: "tester:uqbar".into(),
                            action: kt::VfsAction::GetEntry("/".into()),
                        })?)
                        .send_and_await_response(5)??;

                    let wit::Message::Response((response, _)) = response else { panic!("") };
                    let kt::VfsResponse::GetEntry { children, .. } =
                        serde_json::from_slice(&response.ipc)? else { panic!("") };
                    let mut children: HashSet<_> = children.into_iter().collect();
                    children.remove("/manifest.json");
                    children.remove("/metadata.json");
                    children.remove("/tester.wasm");
                    children.remove("/test_runner.wasm");

                    wit::print_to_terminal(0, &format!("test_runner: running {:?}...", children));

                    for child in &children {
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

                        let (_, response) = Request::new()
                            .target(Address {
                                node: our.node.clone(),
                                process: child_process_id,
                            })?
                            .ipc_bytes(ipc.clone())
                            .send_and_await_response(5)??;

                        let wit::Message::Response((response, _)) = response else { panic!("") };
                        let TesterResponse::Pass = serde_json::from_slice(&response.ipc)? else {
                            return Err(anyhow::anyhow!("{} FAIL", child))
                        };
                    }

                    wit::print_to_terminal(0, &format!("test_runner: done running {:?}", children));

                    Response::new()
                        .ipc_bytes(serde_json::to_vec(&TesterResponse::Pass).unwrap())
                        .send()
                        .unwrap();
                },
                TesterRequest::KernelMessage(_) | TesterRequest::GetFullMessage(_) => { unimplemented!() },
            }
            Ok(())
        },
    }
}

struct Component;
impl Guest for Component {
    fn init(our: String) {
        wit::print_to_terminal(0, "test_runner: begin");

        let our = Address::from_str(&our).unwrap();

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
            match handle_message(&our) {
                Ok(()) => {},
                Err(e) => {
                    wit::print_to_terminal(0, format!(
                        "test_runner: error: {:?}",
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
