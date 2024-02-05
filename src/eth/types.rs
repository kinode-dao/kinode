use crate::types::ProcessId;
use alloy_primitives::{Address, BlockHash, Bytes, ChainId, TxHash, B256, U256};
use alloy_providers::provider::Provider;
use alloy_pubsub::PubSubFrontend;
use alloy_rpc_types::pubsub::{Params, SubscriptionKind, SubscriptionResult};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, CallRequest, Filter, Log};
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
    /// Logs come in as alloy_rpc_types::pubsub::SubscriptionResults
    SubscribeLogs {
        sub_id: u64,
        kind: SubscriptionKind,
        params: Params,
    },
    /// Kill a SubscribeLogs subscription of a given ID, to stop getting updates.
    UnsubscribeLogs(u64),
    /// Raw request. Used by kinode_process_lib.
    Request {
        method: String,
        params: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthResponse {
    Ok,
    Request(serde_json::Value),
    Err(EthError),
    Sub { id: u64, result: SubscriptionResult },
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

//
// Internal types
//

/// For static lifetimes of method strings.
/// Hopefully replaced asap by alloy-rs network abstraction.
pub fn to_static_str(method: &str) -> Option<&'static str> {
    match method {
        "eth_getBalance" => Some("eth_getBalance"),
        "eth_sendRawTransaction" => Some("eth_sendRawTransaction"),
        "eth_call" => Some("eth_call"),
        "eth_getTransactionReceipt" => Some("eth_getTransactionReceipt"),
        "eth_estimateGas" => Some("eth_estimateGas"),
        "eth_blockNumber" => Some("eth_blockNumber"),
        "eth_getBlockByHash" => Some("eth_getBlockByHash"),
        "eth_getBlockByNumber" => Some("eth_getBlockByNumber"),
        "eth_getTransactionByHash" => Some("eth_getTransactionByHash"),
        "eth_getCode" => Some("eth_getCode"),
        "eth_getStorageAt" => Some("eth_getStorageAt"),
        "eth_gasPrice" => Some("eth_gasPrice"),
        "eth_accounts" => Some("eth_accounts"),
        "eth_hashrate" => Some("eth_hashrate"),
        "eth_getLogs" => Some("eth_getLogs"),
        "eth_subscribe" => Some("eth_subscribe"),
        "eth_unsubscribe" => Some("eth_unsubscribe"),
        // "eth_mining" => Some("eth_mining"),
        // "net_version" => Some("net_version"),
        // "net_peerCount" => Some("net_peerCount"),
        // "net_listening" => Some("net_listening"),
        // "web3_clientVersion" => Some("web3_clientVersion"),
        // "web3_sha3" => Some("web3_sha3"),
        _ => None,
    }
}
