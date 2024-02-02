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
    path: "wit",
    world: "process",
    async: true,
});
