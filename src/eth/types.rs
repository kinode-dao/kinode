use crate::types::ProcessId;
use alloy_primitives::{Address, BlockHash, Bytes, ChainId, TxHash, B256, U256};
use alloy_providers::provider::Provider;
use alloy_pubsub::PubSubFrontend;
use alloy_rpc_types::pubsub::{Params, SubscriptionKind, SubscriptionResult};
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag, CallRequest, Filter, Log};
use alloy_transport::RpcResult;
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
    /// get_logs
    /// Vec<Log> or loop through?
    GetLogs {
        filter: Filter,
    },
    /// get_block_number
    GetBlockNumber,
    /// eth_getBalance
    GetBalance {
        address: String, // alloy_primitives::Address deserialization..
        tag: Option<BlockId>,
    },
    GetGasPrice,
    Call {
        tx: CallRequest,
        tag: BlockNumberOrTag,
    },
    GetTransactionCount {
        address: String, // alloy_primitives::Address deserialization..
        tag: Option<BlockNumberOrTag>,
    },
    GetBlockByNumber {
        block: BlockId,
        full_tx: bool,
    },
    GetBlockByHash {
        hash: Vec<u8>, // alloy_primitives::BlockHash deserialization..
        full_tx: bool,
    },
    RawRequest {
        method: String,
        params: Params,
    },
    SendRawTransaction {
        tx: Vec<u8>, // alloy_primitives::Bytes deserialization..
    },
}

/// Potential EthResponse type.
/// Can encapsulate all methods in their own response type,
/// or return generic result which can be parsed later..
#[derive(Debug, Serialize, Deserialize)]
pub enum EthResponse {
    // another possible strat, just return RpcResult<T, E, ErrResp>,
    // then try deserializing on the process_lib side.
    Ok,
    Err(EthError),
    Sub(SubscriptionResult),
    GetLogs(Vec<Log>),
    GetBlockNumber(u64),
    GetBalance(U256),
    GetGasPrice(U256),
    Call(Vec<u8>), // alloy_primimtives::Bytes deserialization..
    GetTransactionCount(U256),
    GetBlockByNumber(Option<Block>),
    GetBlockByHash(Option<Block>),
    // raw json vs enum type vs into T?
    RawRequest(serde_json::Value),
    SendRawTransaction(Vec<u8>), // alloy_primitives::TxHash deserialization..
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
    // todo generics when they work properly: pub struct RpcConnections<T>, where T: Transport
    pub provider: Provider<PubSubFrontend>,
    pub ws_provider_subscriptions: HashMap<(ProcessId, u64), JoinHandle<Result<(), EthError>>>,
}
