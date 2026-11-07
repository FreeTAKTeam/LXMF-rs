//! Additive ZeroMQ custody protocol. Network delivery receipts retain their existing meaning.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Trusted native propagation acknowledgement; neither variant means recipient delivery.
/// This disposition is not accepted as an RPC request field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropagationCustody {
    Packet,
    Resource,
}

impl PropagationCustody {
    pub fn status(self) -> &'static str {
        match self {
            Self::Packet => "sent: propagated",
            Self::Resource => "sent: propagated resource",
        }
    }
}

pub const CAPABILITY: &str = "sdk.capability.durable_broker_v1";
pub use r3akt_event_broker::{
    AckStoredRequest, BrokerBatch, BrokerCheckpoint, BrokerEvent, ConsumerId, EventPosition,
    FetchRequest, JournalId, ResumeRequest, StoredReceipt, MAX_BATCH_BYTES, MAX_BATCH_EVENTS,
};

/// Immutable logical send. `operation_id` is independent of RPC correlation and LXMF wire IDs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmitRequest {
    pub identity: String,
    pub operation_id: String,
    pub destination: String,
    pub title: String,
    pub content: String,
    pub fields: Option<Value>,
    #[serde(default)]
    pub options: crate::rpc::OutboundDeliveryOptions,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileRequest {
    pub identity: String,
    pub operation_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationReceipt {
    pub operation_id: String,
    pub message_id: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnounceProjectionRequest {
    pub identity: String,
    /// Sweep retained latest-per-peer state instead of the legacy latest-32 history window.
    #[serde(default)]
    pub latest_per_peer: bool,
    /// Exclusive peer key; reset to None after completing a sweep.
    #[serde(default)]
    pub after_peer: Option<String>,
}
