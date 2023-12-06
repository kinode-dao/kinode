use serde::{Serialize, Deserialize};

use uqbar_process_lib::Response;
use uqbar_process_lib::kernel_types as kt;
// use uqbar_process_lib::uqbar::process::standard as wit;

type Rsvp = Option<kt::Address>;

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
pub enum TesterRequest {
    Run(Vec<String>),
    KernelMessage(KernelMessage),
    GetFullMessage(kt::Message),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TesterFail {
    pub test: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TesterResponse {
    Pass,
    Fail { test: String, file: String, line: u32, column: u32 },
    GetFullMessage(Option<KernelMessage>),
}

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
pub enum TesterError {
    #[error("RejectForeign")]
    RejectForeign,
    #[error("UnexpectedResponse")]
    UnexpectedResponse,
    #[error("FAIL {test} {message}")]
    Fail { test: String, message: String },
}

#[macro_export]
macro_rules! fail {
    ($test:expr) => {
        Response::new()
            .ipc_bytes(serde_json::to_vec(&tt::TesterResponse::Fail {
                test: $test.into(),
                file: file!().into(),
                line: line!(),
                column: column!(),
            }).unwrap())
            .send()
            .unwrap();
    };
    ($test:expr, $file:expr, $line:expr, $column:expr) => {
        Response::new()
            .ipc_bytes(serde_json::to_vec(&tt::TesterResponse::Fail {
                test: $test.into(),
                file: $file.into(),
                line: $line,
                column: $column,
            }).unwrap())
            .send()
            .unwrap();
        panic!("");
    };
}
