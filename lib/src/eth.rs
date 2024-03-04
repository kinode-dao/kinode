use alloy_rpc_types::pubsub::{Params, SubscriptionKind, SubscriptionResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The Action and Request type that can be made to eth:distro:sys. Any process with messaging
/// capabilities can send this action to the eth provider.
///
/// Will be serialized and deserialized using `serde_json::to_vec` and `serde_json::from_slice`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EthAction {
    /// Subscribe to logs with a custom filter. ID is to be used to unsubscribe.
    /// Logs come in as alloy_rpc_types::pubsub::SubscriptionResults
    SubscribeLogs {
        sub_id: u64,
        chain_id: u64,
        kind: SubscriptionKind,
        params: Params,
    },
    /// Kill a SubscribeLogs subscription of a given ID, to stop getting updates.
    UnsubscribeLogs(u64),
    /// Raw request. Used by kinode_process_lib.
    Request {
        chain_id: u64,
        method: String,
        params: serde_json::Value,
    },
}

/// Incoming `Request` containing subscription updates or errors that processes will receive.
/// Can deserialize all incoming requests from eth:distro:sys to this type.
///
/// Will be serialized and deserialized using `serde_json::to_vec` and `serde_json::from_slice`.
pub type EthSubResult = Result<EthSub, EthSubError>;

/// Incoming type for successful subscription updates.
#[derive(Debug, Serialize, Deserialize)]
pub struct EthSub {
    pub id: u64,
    pub result: SubscriptionResult,
}

/// If your subscription is closed unexpectedly, you will receive this.
#[derive(Debug, Serialize, Deserialize)]
pub struct EthSubError {
    pub id: u64,
    pub error: String,
}

/// The Response type which a process will get from requesting with an [`EthAction`] will be
/// of this type, serialized and deserialized using `serde_json::to_vec`
/// and `serde_json::from_slice`.
///
/// In the case of an [`EthAction::SubscribeLogs`] request, the response will indicate if
/// the subscription was successfully created or not.
#[derive(Debug, Serialize, Deserialize)]
pub enum EthResponse {
    Ok,
    Response { value: serde_json::Value },
    Err(EthError),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum EthError {
    /// provider module cannot parse message
    MalformedRequest,
    /// No RPC provider for the chain
    NoRpcForChain,
    /// Subscription closed
    SubscriptionClosed(u64),
    /// Invalid method
    InvalidMethod(String),
    /// Invalid parameters
    InvalidParams,
    /// Permission denied
    PermissionDenied,
    /// RPC timed out
    RpcTimeout,
    /// RPC gave garbage back
    RpcMalformedResponse,
}

/// The action type used for configuring eth:distro:sys. Only processes which have the "root"
/// capability from eth:distro:sys can successfully send this action.
///
/// NOTE: changes to config will not be persisted between boots, they must be saved in .env
/// to be reflected between boots. TODO: can change this
#[derive(Debug, Serialize, Deserialize)]
pub enum EthConfigAction {
    /// Add a new provider to the list of providers.
    AddProvider(ProviderConfig),
    /// Remove a provider from the list of providers.
    /// The tuple is (chain_id, node_id/rpc_url).
    RemoveProvider((u64, String)),
    /// make our provider public
    SetPublic,
    /// make our provider not-public
    SetPrivate,
    /// add node to whitelist on a provider
    AllowNode(String),
    /// remove node from whitelist on a provider
    UnallowNode(String),
    /// add node to blacklist on a provider
    DenyNode(String),
    /// remove node from blacklist on a provider
    UndenyNode(String),
    /// Set the list of providers to a new list.
    /// Replaces all existing saved provider configs.
    SetProviders(SavedConfigs),
    /// Get the list of current providers as a [`SavedConfigs`] object.
    GetProviders,
    /// Get the current access settings.
    GetAccessSettings,
}

/// Response type from an [`EthConfigAction`] request.
#[derive(Debug, Serialize, Deserialize)]
pub enum EthConfigResponse {
    Ok,
    /// Response from a GetProviders request.
    /// Note the [`crate::core::KnsUpdate`] will only have the correct `name` field.
    /// The rest of the Update is not saved in this module.
    Providers(SavedConfigs),
    /// Response from a GetAccessSettings request.
    AccessSettings(AccessSettings),
    /// Permission denied due to missing capability
    PermissionDenied,
}

/// Settings for our ETH provider
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccessSettings {
    pub public: bool,           // whether or not other nodes can access through us
    pub allow: HashSet<String>, // whitelist for access (only used if public == false)
    pub deny: HashSet<String>,  // blacklist for access (always used)
}

pub type SavedConfigs = Vec<ProviderConfig>;

/// Provider config. Can currently be a node or a ws provider instance.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProviderConfig {
    pub chain_id: u64,
    pub trusted: bool,
    pub provider: NodeOrRpcUrl,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum NodeOrRpcUrl {
    Node {
        kns_update: crate::core::KnsUpdate,
        use_as_provider: bool, // for routers inside saved config
    },
    RpcUrl(String),
}

impl std::cmp::PartialEq<str> for NodeOrRpcUrl {
    fn eq(&self, other: &str) -> bool {
        match self {
            NodeOrRpcUrl::Node { kns_update, .. } => kns_update.name == other,
            NodeOrRpcUrl::RpcUrl(url) => url == other,
        }
    }
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
