use nectar_process_lib::kernel_types as kt;
use nectar_process_lib::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use nectar_process_lib::{
    await_message, call_init, println, Address, Message, ProcessId, Request, Response,
};

wit_bindgen::generate!({
    path: "wit",
    world: "process",
    exports: {
        world: Component,
    },
});

#[derive(Debug, Serialize, Deserialize)]
pub enum ScriptRequest {
    Run {
        package: PackageId,
        wasm_path: String, // vfs path
        args: String,      // first message, in json
    },
    Inject {
        process: String, // ProcessId
        args: String,    // next message, in json
    },
    Terminate(String), // ProcessId string encoded
}

call_init!(init);
fn init(our: Address) {
    println!("script_runner: begin");

    loop {
        match handle_message(&our) {
            Ok(()) => {}
            Err(e) => {
                println!("script_runner: error: {:?}", e);
            }
        };
    }
}

fn handle_message(our: &Address) -> anyhow::Result<()> {
    let message = await_message()?;

    match message {
        Message::Response { .. } => {
            return Err(anyhow::anyhow!("unexpected Response: {:?}", message));
        }
        Message::Request {
            ref source,
            ref body,
            ..
        } => match serde_json::from_slice::<ScriptRequest>(body)? {
            ScriptRequest::Run {
                package,
                wasm_path,
                args,
            } => match handle_run(our, &package, wasm_path, args) {
                Ok(()) => {} // LocalResponse::InstallResponse(InstallResponse::Success),
                Err(_) => {} // LocalResponse::InstallResponse(InstallResponse::Failure),
            },
            ScriptRequest::Inject { process, args } => {
                println!("script_runner: got inject request");
            }
            ScriptRequest::Terminate(process) => {
                println!("script_runner: got terminate request");
            }
        },
    }
    Ok(())
}

fn handle_run(
    our: &Address,
    package: &PackageId,
    wasm_path: String,
    args: String,
) -> anyhow::Result<()> {
    let drive_path = format!("/{}/pkg", package);
    // Request::new()
    //     .target(("our", "vfs", "sys", "nectar"))
    //     .body(serde_json::to_vec(&vfs::VfsRequest {
    //         path: format!("{}/manifest.json", drive_path),
    //         action: vfs::VfsAction::Read,
    //     })?)
    //     .send_and_await_response(5)??;
    // let Some(blob) = get_blob() else {
    //     return Err(anyhow::anyhow!("no blob"));
    // };
    // let manifest = String::from_utf8(blob.bytes)?;
    // let manifest = serde_json::from_str::<Vec<kt::PackageManifestEntry>>(&manifest)?;
    // // always grant read/write to their drive, which we created for them
    // let Some(read_cap) = get_capability(
    //     &Address::new(&our.node, ("vfs", "sys", "nectar")),
    //     &serde_json::to_string(&serde_json::json!({
    //         "kind": "read",
    //         "drive": drive_path,
    //     }))?,
    // ) else {
    //     return Err(anyhow::anyhow!("app store: no read cap"));
    // };
    // let Some(write_cap) = get_capability(
    //     &Address::new(&our.node, ("vfs", "sys", "nectar")),
    //     &serde_json::to_string(&serde_json::json!({
    //         "kind": "write",
    //         "drive": drive_path,
    //     }))?,
    // ) else {
    //     return Err(anyhow::anyhow!("app store: no write cap"));
    // };
    // let Some(networking_cap) = get_capability(
    //     &Address::new(&our.node, ("kernel", "sys", "nectar")),
    //     &"\"network\"".to_string(),
    // ) else {
    //     return Err(anyhow::anyhow!("app store: no net cap"));
    // };
    // first, for each process in manifest, initialize it
    // then, once all have been initialized, grant them requested caps
    // and finally start them.
    let wasm_path = if wasm_path.starts_with("/") {
        wasm_path.clone()
    } else {
        format!("/{}", wasm_path)
    };
    let wasm_path = format!("{}{}", drive_path, wasm_path);
    println!("wasm path: {:?}", wasm_path);
    // build initial caps
    // let mut initial_capabilities: HashSet<kt::Capability> = HashSet::new();
    // if entry.request_networking {
    //     initial_capabilities.insert(kt::de_wit_capability(networking_cap.clone()));
    // }
    // initial_capabilities.insert(kt::de_wit_capability(read_cap.clone()));
    // initial_capabilities.insert(kt::de_wit_capability(write_cap.clone()));
    let process_id = format!("{}:{}", rand::random::<u64>(), package); // all scripts are given random process IDs
    let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
        return Err(anyhow::anyhow!("app store: invalid process id!"));
    };

    // TODO why is this here? Just to make sure that the file exists? I don't think we need it??
    let _bytes_response = Request::new()
        .target(("our", "vfs", "sys", "nectar"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: wasm_path.clone(),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    // if let Some(to_request) = &entry.request_capabilities {
    //     for value in to_request {
    //         let mut capability = None;
    //         match value {
    //             serde_json::Value::String(process_name) => {
    //                 if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
    //                     capability = get_capability(
    //                         &Address {
    //                             node: our.node.clone(),
    //                             process: parsed_process_id.clone(),
    //                         },
    //                         "\"messaging\"".into(),
    //                     );
    //                 }
    //             }
    //             serde_json::Value::Object(map) => {
    //                 if let Some(process_name) = map.get("process") {
    //                     if let Ok(parsed_process_id) = process_name
    //                         .as_str()
    //                         .unwrap_or_default()
    //                         .parse::<ProcessId>()
    //                     {
    //                         if let Some(params) = map.get("params") {
    //                             if params.to_string() == "\"root\"" {
    //                                 println!("app-store: app requested root capability, ignoring");
    //                                 continue;
    //                             }

    //                             capability = get_capability(
    //                                 &Address {
    //                                     node: our.node.clone(),
    //                                     process: parsed_process_id.clone(),
    //                                 },
    //                                 &params.to_string(),
    //                             );
    //                         }
    //                     }
    //                 }
    //             }
    //             _ => {
    //                 continue;
    //             }
    //         }
    //         if let Some(cap) = capability {
    //             initial_capabilities.insert(kt::de_wit_capability(cap));
    //         } else {
    //             println!(
    //                 "app-store: no cap: {}, for {} to request!",
    //                 value.to_string(),
    //                 package
    //             );
    //         }
    //     }
    // }
    Request::new()
        .target(("our", "kernel", "sys", "nectar"))
        .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
            id: parsed_new_process_id.clone(),
            wasm_bytes_handle: wasm_path,
            wit_version: None,
            on_exit: kt::OnExit::None,
            initial_capabilities: HashSet::new(), // TODO
            public: true, // TODO unclear if this should be public or not...definitley gets around grant_caps issues
        })?)
        .inherit(true)
        .send_and_await_response(5)??;
    // if let Some(to_grant) = &entry.grant_capabilities {
    //     for value in to_grant {
    //         match value {
    //             serde_json::Value::String(process_name) => {
    //                 if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
    //                     let _ = Request::new()
    //                         .target(("our", "kernel", "sys", "nectar"))
    //                         .body(
    //                             serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
    //                                 target: parsed_process_id,
    //                                 capabilities: vec![kt::Capability {
    //                                     issuer: Address {
    //                                         node: our.node.clone(),
    //                                         process: parsed_new_process_id.clone(),
    //                                     },
    //                                     params: "\"messaging\"".into(),
    //                                 }],
    //                             })
    //                             .unwrap(),
    //                         )
    //                         .send()?;
    //                 }
    //             }
    //             serde_json::Value::Object(map) => {
    //                 if let Some(process_name) = map.get("process") {
    //                     if let Ok(parsed_process_id) = process_name
    //                         .as_str()
    //                         .unwrap_or_default()
    //                         .parse::<ProcessId>()
    //                     {
    //                         if let Some(params) = map.get("params") {
    //                             let _ = Request::new()
    //                                 .target(("our", "kernel", "sys", "nectar"))
    //                                 .body(
    //                                     serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
    //                                         target: parsed_process_id,
    //                                         capabilities: vec![kt::Capability {
    //                                             issuer: Address {
    //                                                 node: our.node.clone(),
    //                                                 process: parsed_new_process_id.clone(),
    //                                             },
    //                                             params: params.to_string(),
    //                                         }],
    //                                     })
    //                                     .unwrap(),
    //                                 )
    //                                 .send()?;
    //                         }
    //                     }
    //                 }
    //             }
    //             _ => {
    //                 continue;
    //             }
    //         }
    //     }
    // }
    Request::new()
        .target(("our", "kernel", "sys", "nectar"))
        .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
            parsed_new_process_id,
        ))?)
        .send_and_await_response(5)??;
    Ok(())
}
