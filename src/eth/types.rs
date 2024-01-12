use ethers::prelude::Provider;
use ethers::types::{Filter, Log, U256};
use ethers_providers::{Middleware, Ws};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::task::JoinHandle;

/// The Request type that can be made to eth:sys:nectar. Currently primitive, this
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
    pub ws_rpc_url: String,
    pub ws_provider_subscriptions: HashMap<u64, WsProviderSubscription>,
}

pub struct WsProviderSubscription {
    pub handle: JoinHandle<()>,
    pub provider: Provider<Ws>,
    pub subscription: U256,
}

impl WsProviderSubscription {
    pub async fn kill(&self) {
        let _ = self.provider.unsubscribe(self.subscription).await;
        self.handle.abort();
    }
}
