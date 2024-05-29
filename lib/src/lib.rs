#![feature(let_chains)]

pub mod core;
pub mod eth;
mod http;

pub mod types {
    pub use crate::core;
    pub use crate::eth;
    pub use crate::http::client_types as http_client;
    pub use crate::http::server_types as http_server;
}

pub use kinode::process;
pub use kinode::process::standard as wit;

wasmtime::component::bindgen!({
    path: "wit-v0.7.0",
    world: "process",
    async: true,
});

pub mod v0 {
    pub use kinode::process;
    pub use kinode::process::standard as wit;
    wasmtime::component::bindgen!({
        path: "wit-v0.8.0",
        world: "process-v0",
        async: true,
    });
}
