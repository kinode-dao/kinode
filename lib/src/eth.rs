use alloy::transports::Authorization as AlloyAuthorization;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Subscription kind. Pulled directly from alloy (https://github.com/alloy-rs/alloy).
/// Why? Because alloy is not yet 1.0 and the types in this interface must be stable.
/// If alloy SubscriptionKind changes, we can implement a transition function in runtime
/// for this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionKind {
    /// New block headers subscription.
    ///
    /// Fires a notification each time a new header is appended to the chain, including chain
    /// reorganizations. In case of a chain reorganization the subscription will emit all new
    /// headers for the new chain. Therefore the subscription can emit multiple headers on the same
    /// height.
    NewHeads,
    /// Logs subscription.
    ///
    /// Returns logs that are included in new imported blocks and match the given filter criteria.
    /// In case of a chain reorganization previous sent logs that are on the old chain will be
    /// resent with the removed property set to true. Logs from transactions that ended up in the
    /// new chain are emitted. Therefore, a subscription can emit logs for the same transaction
    /// multiple times.
    Logs,
    /// New Pending Transactions subscription.
    ///
    /// Returns the hash or full tx for all transactions that are added to the pending state and
    /// are signed with a key that is available in the node. When a transaction that was
    /// previously part of the canonical chain isn't part of the new canonical chain after a
    /// reorganization its again emitted.
    NewPendingTransactions,
    /// Node syncing status subscription.
    ///
    /// Indicates when the node starts or stops synchronizing. The result can either be a boolean
    /// indicating that the synchronization has started (true), finished (false) or an object with
    /// various progress indicators.
    Syncing,
}

/// The Action and Request type that can be made to eth:distro:sys. Any process with messaging
/// capabilities can send this action to the eth provider.
///
/// Will be serialized and deserialized using [`serde_json::to_vec`] and [`serde_json::from_slice`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EthAction {
    /// Subscribe to logs with a custom filter. ID is to be used to unsubscribe.
    /// Logs come in as JSON value which can be parsed to [`alloy::rpc::types::eth::pubsub::SubscriptionResult`]
    SubscribeLogs {
        sub_id: u64,
        chain_id: u64,
        kind: SubscriptionKind,
        params: serde_json::Value,
    },
    /// Kill a SubscribeLogs subscription of a given ID, to stop getting updates.
    UnsubscribeLogs(u64),
    /// Raw request. Used by hyperware_process_lib.
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EthSub {
    pub id: u64,
    /// can be parsed to [`alloy::rpc::types::eth::pubsub::SubscriptionResult`]
    pub result: serde_json::Value,
}

/// If your subscription is closed unexpectedly, you will receive this.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EthSubError {
    pub id: u64,
    pub error: String,
}

/// The Response body type which a process will get from requesting
/// with an [`EthAction`] will be of this type, serialized and deserialized
/// using [`serde_json::to_vec`] and [`serde_json::from_slice`].
///
/// In the case of an [`EthAction::SubscribeLogs`] request, the response will indicate if
/// the subscription was successfully created or not.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum EthResponse {
    Ok,
    /// Value will be a JSON-RPC standard item
    Response(serde_json::Value),
    Err(EthError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EthError {
    /// RPC provider returned an error.
    /// Can be parsed to [`alloy::rpc::json_rpc::ErrorPayload`]
    RpcError(serde_json::Value),
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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// Get the state of calls and subscriptions. Used for debugging.
    GetState,
}

/// Response type from an [`EthConfigAction`] request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum EthConfigResponse {
    Ok,
    /// Response from a GetProviders request.
    /// Note the [`crate::core::HnsUpdate`] will only have the correct `name` field.
    /// The rest of the Update is not saved in this module.
    Providers(SavedConfigs),
    /// Response from a GetAccessSettings request.
    AccessSettings(AccessSettings),
    /// Permission denied due to missing capability
    PermissionDenied,
    /// Response from a GetState request
    State {
        active_subscriptions: HashMap<crate::core::Address, HashMap<u64, Option<String>>>, // None if local, Some(node_provider_name) if remote
        outstanding_requests: HashSet<u64>,
    },
}

/// Settings for our ETH provider
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccessSettings {
    /// whether or not other nodes can access through us
    pub public: bool,
    /// whitelist for access (only used if public == false)
    pub allow: HashSet<String>,
    /// blacklist for access (always used)
    pub deny: HashSet<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SavedConfigs(pub Vec<ProviderConfig>);

impl SavedConfigs {
    /// insert while enforcing that each config is unique
    pub fn insert(&mut self, index: usize, config: ProviderConfig) {
        // filter out any configs which are the same as incoming config
        if let NodeOrRpcUrl::RpcUrl { ref url, .. } = config.provider {
            // remove old occurrences with matching URLs to make sure
            //  auth is what was passed in and never an old value
            self.0.retain(|c| {
                if let NodeOrRpcUrl::RpcUrl {
                    url: ref old_url, ..
                } = c.provider
                {
                    return url != old_url;
                }
                c != &config
            });
        } else {
            self.0.retain(|c| c != &config);
        }
        self.0.insert(index, config);
    }
}

/// Provider config. Can currently be a node or a ws provider instance.
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct ProviderConfig {
    pub chain_id: u64,
    pub trusted: bool,
    pub provider: NodeOrRpcUrl,
}

#[derive(Clone, Debug, Serialize, Hash, Eq, PartialEq)]
pub enum NodeOrRpcUrl {
    Node {
        hns_update: crate::core::HnsUpdate,
        use_as_provider: bool, // false for just-routers inside saved config
    },
    RpcUrl {
        url: String,
        auth: Option<Authorization>,
    },
}

impl std::cmp::PartialEq<str> for NodeOrRpcUrl {
    fn eq(&self, other: &str) -> bool {
        match self {
            NodeOrRpcUrl::Node { hns_update, .. } => hns_update.name == other,
            NodeOrRpcUrl::RpcUrl { url, .. } => url == other,
        }
    }
}

impl<'de> Deserialize<'de> for NodeOrRpcUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RpcUrlHelper {
            String(String),
            Struct {
                url: String,
                auth: Option<Authorization>,
            },
        }

        #[derive(Deserialize)]
        enum Helper {
            Node {
                hns_update: crate::core::HnsUpdate,
                use_as_provider: bool,
            },
            RpcUrl(RpcUrlHelper),
        }

        let helper = Helper::deserialize(deserializer)?;

        Ok(match helper {
            Helper::Node {
                hns_update,
                use_as_provider,
            } => NodeOrRpcUrl::Node {
                hns_update,
                use_as_provider,
            },
            Helper::RpcUrl(url_helper) => match url_helper {
                RpcUrlHelper::String(url) => NodeOrRpcUrl::RpcUrl { url, auth: None },
                RpcUrlHelper::Struct { url, auth } => NodeOrRpcUrl::RpcUrl { url, auth },
            },
        })
    }
}

#[derive(Deserialize, Serialize)]
pub struct RpcUrlConfigInput {
    pub url: String,
    pub auth: Option<Authorization>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub enum Authorization {
    Basic(String),
    Bearer(String),
    Raw(String),
}

impl From<Authorization> for AlloyAuthorization {
    fn from(auth: Authorization) -> AlloyAuthorization {
        match auth {
            Authorization::Basic(value) => AlloyAuthorization::Basic(value),
            Authorization::Bearer(value) => AlloyAuthorization::Bearer(value),
            Authorization::Raw(value) => AlloyAuthorization::Raw(value),
        }
    }
}
