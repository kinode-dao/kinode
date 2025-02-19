use crate::types::core::{Address, Identity, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Must be parsed from message pack vector.
/// all Get actions must be sent from local process. used for debugging
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetAction {
    /// Received from a router of ours when they have a new pending passthrough for us.
    /// We should respond (if we desire) by using them to initialize a routed connection
    /// with the NodeId given.
    ConnectionRequest(NodeId),
    /// can only receive from trusted source: requires net root cap
    HnsUpdate(HnsUpdate),
    /// can only receive from trusted source: requires net root cap
    HnsBatchUpdate(Vec<HnsUpdate>),
    /// get a list of peers we are connected to
    GetPeers,
    /// get the [`Identity`] struct for a single peer
    GetPeer(String),
    /// get a user-readable diagnostics string containing networking inforamtion
    GetDiagnostics,
    /// sign the attached blob payload, sign with our node's networking key.
    /// **only accepted from our own node**
    /// **the source [`Address`] will always be prepended to the payload**
    Sign,
    /// given a message in blob payload, verify the message is signed by
    /// the given source. if the signer is not in our representation of
    /// the PKI, will not verify.
    /// **the `from` [`Address`] will always be prepended to the payload**
    Verify { from: Address, signature: Vec<u8> },
}

/// Must be parsed from message pack vector
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetResponse {
    /// response to [`NetAction::ConnectionRequest`]
    Accepted(NodeId),
    /// response to [`NetAction::ConnectionRequest`]
    Rejected(NodeId),
    /// response to [`NetAction::GetPeers`]
    Peers(Vec<Identity>),
    /// response to [`NetAction::GetPeer`]
    Peer(Option<Identity>),
    /// response to [`NetAction::GetDiagnostics`]. a user-readable string.
    Diagnostics(String),
    /// response to [`NetAction::Sign`]. contains the signature in blob
    Signed,
    /// response to [`NetAction::Verify`]. boolean indicates whether
    /// the signature was valid or not. note that if the signer node
    /// cannot be found in our representation of PKI, this will return false,
    /// because we cannot find the networking public key to verify with.
    Verified(bool),
}

//
// HNS parts of the networking protocol
//

#[derive(Clone, Debug, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct HnsUpdate {
    pub name: String,
    pub public_key: String,
    pub ips: Vec<String>,
    pub ports: BTreeMap<String, u16>,
    pub routers: Vec<String>,
}

impl HnsUpdate {
    pub fn get_protocol_port(&self, protocol: &str) -> Option<&u16> {
        self.ports.get(protocol)
    }
}
