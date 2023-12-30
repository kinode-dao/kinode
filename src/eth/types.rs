use crate::http::types::HttpServerRequest;
use crate::types::*;
use dashmap::DashMap;
use ethers::prelude::Provider;
use ethers::types::{Filter, U256};
use ethers_providers::{Http, Middleware, Ws};
use futures::stream::SplitSink;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type WsRequestIds = Arc<DashMap<u32, u32>>;

pub struct WsProviderSubscription {
    pub handle: Option<JoinHandle<()>>,
    pub provider: Option<Provider<Ws>>,
    pub subscription: Option<U256>,
}

impl Default for WsProviderSubscription {
    fn default() -> Self {
        Self {
            handle: None,
            provider: None,
            subscription: None,
        }
    }
}

impl WsProviderSubscription {
    pub async fn kill(&self) -> () {
        if let Some(provider) = &self.provider {
            if let Some(subscription) = &self.subscription {
                provider.unsubscribe(subscription).await;
            }
        }
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

pub struct RpcConnections {
    pub ws_sender:
        Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TungsteniteMessage>>,
    pub ws_sender_ids: WsRequestIds,
    pub ws_rpc_url: Option<String>,
    pub ws_provider_subscriptions: HashMap<u64, WsProviderSubscription>,
    pub http_rpc_url: Option<String>,
    pub uq_provider: Option<NodeId>,
    pub ws_provider: Option<Provider<Ws>>,
    pub http_provider: Option<Provider<Http>>,
}

impl Default for RpcConnections {
    fn default() -> Self {
        Self {
            ws_sender: None,
            ws_sender_ids: Arc::new(DashMap::new()),
            ws_provider: None,
            ws_provider_subscriptions: HashMap::<u64, WsProviderSubscription>::new(),
            http_provider: None,
            uq_provider: None,
            http_rpc_url: None,
            ws_rpc_url: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthRpcError {
    NoRsvp,
    BadJson,
    NoJson,
    EventSubscriptionFailed,
}
impl EthRpcError {
    pub fn _kind(&self) -> &str {
        match *self {
            EthRpcError::NoRsvp { .. } => "NoRsvp",
            EthRpcError::BadJson { .. } => "BapJson",
            EthRpcError::NoJson { .. } => "NoJson",
            EthRpcError::EventSubscriptionFailed { .. } => "EventSubscriptionFailed",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubscribeLogs {
    pub filter: Filter,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum EthRequest {
    SubscribeLogs(SubscribeLogs),
    UnsubscribeLogs(u64),
}

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
    HttpServerRequest(HttpServerRequest),
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
