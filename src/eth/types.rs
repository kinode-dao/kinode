use crate::types::ProcessId;
use ethers::prelude::Provider;
use ethers::types::{Filter, Log};
use ethers_providers::Ws;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::task::JoinHandle;

/// The Request type that can be made to eth:distro:sys. Currently primitive, this
/// enum will expand to support more actions in the future.
///
/// Will be serialized and deserialized using `serde_json::to_vec` and `serde_json::from_slice`.
#[derive(Debug, Serialize, Deserialize)]
pub enum EthAction {
    /// Subscribe to logs with a custom filter. ID is to be used to unsubscribe.
    SubscribeLogs { sub_id: u64, filter: Filter },
    /// Kill a SubscribeLogs subscription of a given ID, to stop getting updates.
    UnsubscribeLogs(u64),
}

/// The Response type which a process will get from requesting with an [`EthAction`] will be
/// of the form `Result<(), EthError>`, serialized and deserialized using `serde_json::to_vec`
/// and `serde_json::from_slice`.
#[derive(Debug, Serialize, Deserialize)]
pub enum EthError {
    /// The ethers provider threw an error when trying to subscribe
    /// (contains ProviderError serialized to debug string)
    ProviderError(String),
    SubscriptionClosed,
    /// The subscription ID was not found, so we couldn't unsubscribe.
    SubscriptionNotFound,
}

/// The Request type which a process will get from using SubscribeLogs to subscribe
/// to a log.
///
/// Will be serialized and deserialized using `serde_json::to_vec` and `serde_json::from_slice`.
#[derive(Debug, Serialize, Deserialize)]
pub enum EthSubEvent {
    Log(Log),
}

//
// Internal types
//

/// Primary state object of the `eth` module
pub struct RpcConnections {
    pub provider: Provider<Ws>,
    pub ws_provider_subscriptions: HashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>,
}
