#![feature(let_chains)]
use uqbar_process_lib::{
    await_message, http::bind_http_static_path, http::HttpServerError, println, Address,
    Message, ProcessId,
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;

const HOME_PAGE: &str = include_str!("home.html");

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!("homepage: ended with error: {:?}", e);
            }
        }
    }
}

fn main(our: Address) -> anyhow::Result<()> {
    // bind to root path on http_server (we have special dispensation to do so!)
    bind_http_static_path(
        "/",
        true,
        false,
        Some("text/html".to_string()),
        HOME_PAGE
            .replace("${our}", &our.node)
            .to_string()
            .as_bytes()
            .to_vec(),
    )?;

    bind_http_static_path(
        "/our",
        false,
        false,
        Some("text/html".to_string()),
        our.node.clone().as_bytes().to_vec(),
    )?;

    bind_http_static_path(
        "/our.js",
        false,
        false,
        Some("application/javascript".to_string()),
        format!("window.our = {{}}; window.our.node = '{}';", &our.node).as_bytes().to_vec(),
    )?;

    loop {
        let Ok(ref message) = await_message() else {
            println!("homepage: got network error??");
            continue;
        };
        if let Message::Response { source, ipc, ..} = message
            && source.process == "http_server:sys:uqbar"
        {
            match serde_json::from_slice::<Result<(), HttpServerError>>(&ipc) {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => println!("homepage: got error from http_server: {e}"),
                Err(_e) => println!("homepage: got malformed message from http_server!"),
            }
        } else {
            println!("homepage: got message: {message:?}");
            //println!("homepage: got message from {source:?}: {message:?}");
        }
    }
}
