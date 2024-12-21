use crate::types::core::ProcessId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum FdManagerRequest {
    /// other process -> fd-manager
    /// must send this to fd-manager to get an initial fds_limit
    RequestFdsLimit,
    /// other process -> fd-manager
    /// send this to notify fd-manager that limit was hit,
    /// which may or may not be reacted to
    FdsLimitHit,

    /// fd-manager -> other process
    FdsLimit(u64),

    /// administrative
    UpdateMaxFdsAsFractionOfUlimitPercentage(u64),
    /// administrative
    UpdateUpdateUlimitSecs(u64),
    /// administrative
    UpdateCullFractionDenominator(u64),

    /// get a `HashMap` of all `ProcessId`s to their number of allocated file descriptors.
    GetState,
    /// get the `u64` number of file descriptors allocated to `ProcessId`.
    GetProcessFdLimit(ProcessId),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum FdManagerResponse {
    /// response to [`FdManagerRequest::GetState`]
    GetState(HashMap<ProcessId, FdsLimit>),
    /// response to [`FdManagerRequest::GetProcessFdLimit`]
    GetProcessFdLimit(u64),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct FdsLimit {
    pub limit: u64,
    pub hit_count: u64,
}

#[derive(Debug, Error)]
pub enum FdManagerError {
    #[error("fd-manager: received a non-Request message")]
    NotARequest,
    #[error("fd-manager: received a non-FdManangerRequest")]
    BadRequest,
    #[error("fd-manager: received a FdManagerRequest::FdsLimit, but I am the one who sets limits")]
    FdManagerWasSentLimit,
}
