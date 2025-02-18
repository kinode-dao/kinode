#[allow(unused_imports)]
use crate::hyperware::process::tester::{FailResponse, Response as TesterResponse};

#[macro_export]
macro_rules! fail {
    ($test:expr) => {
        Response::new()
            .body(TesterResponse::Run(Err(FailResponse {
                test: $test.into(),
                file: file!().into(),
                line: line!(),
                column: column!(),
            })))
            .send()
            .unwrap();
        panic!("")
    };
    ($test:expr, $file:expr, $line:expr, $column:expr) => {
        Response::new()
            .body(TesterResponse::Run(Err(FailResponse {
                test: $test.into(),
                file: $file.into(),
                line: $line,
                column: $column,
            })))
            .send()
            .unwrap();
        panic!("")
    };
}
