use serde::{Deserialize, Serialize};

use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::Address;

type Rsvp = Option<Address>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KernelMessage {
    pub id: u64,
    pub source: Address,
    pub target: Address,
    pub rsvp: Rsvp,
    pub message: kt::Message,
    pub lazy_load_blob: Option<kt::LazyLoadBlob>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TesterRequest {
    Run {
        input_node_names: Vec<String>,
        test_names: Vec<String>,
        test_timeout: u64,
    },
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
    Fail {
        test: String,
        file: String,
        line: u32,
        column: u32,
    },
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
            .body(
                serde_json::to_vec(&tt::TesterResponse::Fail {
                    test: $test.into(),
                    file: file!().into(),
                    line: line!(),
                    column: column!(),
                })
                .unwrap(),
            )
            .send()
            .unwrap();
        panic!("")
    };
    ($test:expr, $file:expr, $line:expr, $column:expr) => {
        Response::new()
            .body(
                serde_json::to_vec(&tt::TesterResponse::Fail {
                    test: $test.into(),
                    file: $file.into(),
                    line: $line,
                    column: $column,
                })
                .unwrap(),
            )
            .send()
            .unwrap();
        panic!("")
    };
}
