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

struct EthAccounts {
    addresses: Option<Vec<String>>,
}

struct EthBlockNumber {

}

enum DebugMethod {
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

enum EthMethod {
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

enum NetMethod {
    Listening,
    PeerCount,
    Version,
}

enum TraceMethod {
    Call,
    CallMany,
    Get,
    RawTransaction,
    ReplayBlockTransactions,
    ReplayTransaction,
    Transaction,
}

enum TxPoolMethod {
    Content,
    Inspect,
    Status,
}