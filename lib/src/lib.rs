pub mod core;
pub mod eth;
mod fd_manager;
mod http;
mod kernel;
mod kv;
mod net;
mod sqlite;
mod state;
mod timer;
mod vfs;

pub mod types {
    pub use crate::core;
    pub use crate::eth;
    pub use crate::http::client_types as http_client;
    pub use crate::http::server_types as http_server;
}

// `trappable_imports: true` keeps behavior the same as pre-240410
//  where imports are wrapped with an `anyhow::Result`.
//  This allows errors that occur in imports to be handled naturally,
//  namely by printing to terminal, e.g.
//  https://github.com/hyperware-ai/hyperware/blob/b75cf2c0f9f274edcaa43449b460e6ba11d852a9/hyperware/src/kernel/process.rs#L381
//
//  source:
//  https://github.com/bytecodealliance/wasmtime/commit/1cf0060bbc17aaf35b81b989c6394df254bb4f2e

pub mod v1 {
    pub use hyperware::process;
    pub use hyperware::process::standard as wit;
    wasmtime::component::bindgen!({
        path: "wit-v1.0.0",
        world: "process-v1",
        async: true,
        trappable_imports: true,
    });
}
