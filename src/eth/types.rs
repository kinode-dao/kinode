use crate::http::types::HttpServerAction;
use ethers::types::{ValueOrArray, U256, U64};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct EthEventSubscription {
    addresses: Option<Vec<String>>,
    from_block: Option<u64>,
    to_block: Option<u64>,
    events: Option<Vec<String>>, // aka topic0s
    topic1: Option<U256>,
    topic2: Option<U256>,
    topic3: Option<U256>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ProviderAction {
    HttpServerAction(HttpServerAction),
    EthRpcAction(EthRpcAction),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthRpcAction {
    JsonRpcRequest(String),
    Eth(EthMethod),
    Debug(DebugMethod),
    Net(NetMethod),
    Trace(TraceMethod),
    TxPool(TxPoolMethod),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DebugMethod {
    GetRawBlock,
    GetRawHeader,
    GetRawReceipts,
    GetRawTransaction,
    TraceBlock,
    TraceBlockByHash,
    TraceBlockByNumber,
    TraceCall,
    TraceCallMany,
    TraceTransaction,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthMethod {
    Accounts,
    BlockNumber,
    Call,
    ChainId,
    CreateAccessList,
    EstimateGas,
    FeeHistory,
    GasPrice,
    GetBalance,
    GetBlockByHash,
    GetBlockByNumber,
    GetBlockReceipts,
    GetBlockTransactionCountByHash,
    GetBlockTransactionCountByNumber,
    GetCode,
    GetFilterChanges,
    GetFilterLogs,
    GetLogs,
    GetStorageAt,
    GetTransactionByBlockHashAndIndex,
    GetTransactionByBlockNumberAndIndex,
    GetTransactionByHash,
    GetTransactionCount,
    GetTransactionReceipt,
    GetUncleByBlockHashAndIndex,
    GetUncleByBlockNumberAndIndex,
    GetUncleCountByBlockHash,
    GetUncleCountByBlockNumber,
    MaxPriorityFeePerGas,
    Mining,
    NewBlockFilter,
    NewFilter,
    NewPendingTransactionFilter,
    ProtocolVersion,
    SendRawTransaction,
    SendTransaction,
    Sign,
    SignTransaction,
    SignTypedData,
    Subscribe,
    Syncing,
    UninstallFilter,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum NetMethod {
    Listening,
    PeerCount,
    Version,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TraceMethod {
    Call,
    CallMany,
    Get,
    RawTransaction,
    ReplayBlockTransactions,
    ReplayTransaction,
    Transaction,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TxPoolMethod {
    Content,
    Inspect,
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthProviderError {
    NoRsvp,
    BadJson,
    NoJson,
    EventSubscriptionFailed,
}

impl EthProviderError {
    pub fn _kind(&self) -> &str {
        match *self {
            EthProviderError::NoRsvp { .. } => "NoRsvp",
            EthProviderError::BadJson { .. } => "BapJson",
            EthProviderError::NoJson { .. } => "NoJson",
            EthProviderError::EventSubscriptionFailed { .. } => "EventSubscriptionFailed",
        }
    }
}
