#![feature(let_chains)]
use uqbar_process_lib::{
    grant_messaging, http::bind_http_static_path, http::HttpServerError, println, receive, Address,
    Message, ProcessId, Response,
};

wit_bindgen::generate!({
    path: "../../wit",
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
        grant_messaging(&our, vec![ProcessId::new(Some("http_server"), "sys", "uqbar")]);
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

    loop {
        let Ok((ref source, ref message)) = receive() else {
            println!("homepage: got network error??");
            continue;
        };
        if let Message::Response((ref msg, _)) = message
            && source.process == "http_server:sys:uqbar"
        {
            match serde_json::from_slice::<Result<(), HttpServerError>>(&msg.ipc) {
                Ok(Ok(())) => continue,
                Ok(Err(e)) => println!("homepage: got error from http_server: {e}"),
                Err(_e) => println!("homepage: got malformed message from http_server!"),
            }
        } else {
            println!("homepage: got message from {source:?}: {message:?}");
        }
    }
}
