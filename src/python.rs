use wasi_common::pipe::{ReadPipe, WritePipe};
use wasmtime::{Config, Engine, Linker, Module, Store};
use wasmtime_wasi::WasiCtxBuilder;

use crate::types::{
    Address, KernelMessage, LazyLoadBlob, Message, MessageReceiver, MessageSender, PythonRequest,
    PythonResponse, Request, Response, PYTHON_PROCESS_ID,
};

include!("python_includes.rs");

pub async fn python(
    our_node: String,
    send_to_loop: MessageSender,
    mut recv_from_loop: MessageReceiver,
) -> anyhow::Result<()> {
    loop {
        let km = recv_from_loop.recv().await.unwrap();
        let KernelMessage {
            id,
            source,
            rsvp,
            message,
            lazy_load_blob,
            ..
        } = km;
        if our_node != source.node {
            println!(
                "python: Request must come from our_node={}, got: {}\r",
                our_node, source.node,
            );
            continue;
        }
        let Message::Request(Request { ref body, .. }) = message else {
            println!("python: got a non-Request\r");
            continue;
        };
        let Ok(PythonRequest::Run) = serde_json::from_slice(body) else {
            println!("python: got a non-Run Request\r");
            continue;
        };
        let Some(lazy_load_blob) = lazy_load_blob else {
            println!("python: Run Request must contain a lazy_load_blob\r");
            continue;
        };
        let Ok(code) = String::from_utf8(lazy_load_blob.bytes) else {
            println!("python: got a bad `code` in `blob` (must be utf-8)\r");
            continue;
        };
        let target = rsvp.unwrap_or_else(|| source);

        let our_node = our_node.clone();
        let send_to_loop = send_to_loop.clone();
        tokio::spawn(async move {
            let message = match run_python(&code).await {
                Ok(output) => make_output_message(our_node, id, target, output),
                Err(e) => make_error_message(our_node, id, target, format!("{:?}", e)),
            };
            let _ = send_to_loop.send(message).await;
        });
    }
}

async fn run_python(code: &str) -> anyhow::Result<Vec<u8>> {
    let wasi_stdin = ReadPipe::from(code);
    let wasi_stdout = WritePipe::new_in_memory();
    let wasi_stderr = WritePipe::new_in_memory();

    let result = {
        // Define the WASI functions globally on the `Config`.
        let mut config = Config::new();
        config.async_support(true);
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;

        // uncomment to bring in non-stdlib libs (except those containing C code)
        // let dir = cap_std::fs::Dir::open_ambient_dir(
        //     "venv",
        //     cap_std::ambient_authority(),
        // ).unwrap();

        // Create a WASI context and put it in a Store; all instances in the store
        // share this context. `WasiCtxBuilder` provides a number of ways to
        // configure what the target program will have access to.
        let wasi = WasiCtxBuilder::new()
            // uncomment to bring in non-stdlib libs (except those containing C code)
            // .preopened_dir(dir, "/venv")?
            // .env("PYTHONPATH", "/venv/lib/python3.12/site-packages")?
            .stdin(Box::new(wasi_stdin.clone()))
            .stdout(Box::new(wasi_stdout.clone()))
            .stderr(Box::new(wasi_stderr.clone()))
            .build();
        let mut store = Store::new(&engine, wasi);

        // Instantiate our module with the imports we've created, and run it.
        let module = Module::from_binary(&engine, PYTHON_WASM)?;
        linker.module_async(&mut store, "", &module).await?;

        linker
            .get_default(&mut store, "")?
            .typed::<(), ()>(&store)?
            .call_async(&mut store, ())
            .await
    };

    let contents: Vec<u8> = match result {
        Ok(_) => {
            wasi_stdout
                .try_into_inner()
                .expect("sole remaining reference to WritePipe")
                .into_inner()
        }
        Err(_) => {
            wasi_stderr
                .try_into_inner()
                .expect("sole remaining reference to WritePipe")
                .into_inner()
        }
    };

    Ok(contents)
}

fn make_output_message(
    our_node: String,
    id: u64,
    target: Address,
    output: Vec<u8>,
) -> KernelMessage {
    KernelMessage {
        id,
        source: Address {
            node: our_node,
            process: PYTHON_PROCESS_ID.clone(),
        },
        target,
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                body: serde_json::to_vec(&PythonResponse::Run).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: Some(LazyLoadBlob {
            mime: None,
            bytes: output,
        }),
    }
}

fn make_error_message(our_node: String, id: u64, target: Address, error: String) -> KernelMessage {
    KernelMessage {
        id,
        source: Address {
            node: our_node,
            process: PYTHON_PROCESS_ID.clone(),
        },
        target,
        rsvp: None,
        message: Message::Response((
            Response {
                inherit: false,
                body: serde_json::to_vec(&PythonResponse::Err(error)).unwrap(),
                metadata: None,
                capabilities: vec![],
            },
            None,
        )),
        lazy_load_blob: None,
    }
}
