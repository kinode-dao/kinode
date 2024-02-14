use alloy_rpc_types::pubsub::{Params, SubscriptionKind, SubscriptionResult};
use serde::{Deserialize, Serialize};

/// The Action and Request type that can be made to eth:distro:sys.
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
/// Incoming subscription update.
#[derive(Debug, Serialize, Deserialize)]
pub struct EthSub {
    pub id: u64,
    pub result: SubscriptionResult,
}

/// The Response type which a process will get from requesting with an [`EthAction`] will be
/// of the form `Result<(), EthError>`, serialized and deserialized using `serde_json::to_vec`
/// and `serde_json::from_slice`.
#[derive(Debug, Serialize, Deserialize)]
pub enum EthResponse {
    Ok,
    Response { value: serde_json::Value },
    Err(EthError),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthError {
    /// Underlying transport error
    TransportError(String),
    /// Subscription closed
    SubscriptionClosed(u64),
    /// The subscription ID was not found, so we couldn't unsubscribe.
    SubscriptionNotFound,
    /// Invalid method
    InvalidMethod(String),
    /// Permission denied
    PermissionDenied(String),
    /// Internal RPC error
    RpcError(String),
}

//
// Internal types
//

/// For static lifetimes of method strings.
/// Replaced soon by alloy-rs network abstraction.
pub fn to_static_str(method: &str) -> Option<&'static str> {
    match method {
        "eth_getBalance" => Some("eth_getBalance"),
        "eth_sendRawTransaction" => Some("eth_sendRawTransaction"),
        "eth_call" => Some("eth_call"),
        "eth_chainId" => Some("eth_chainId"),
        "eth_getTransactionReceipt" => Some("eth_getTransactionReceipt"),
        "eth_getTransactionCount" => Some("eth_getTransactionCount"),
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

pub enum ProviderInput {
    WS(String),
    Node(String),
}
