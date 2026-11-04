use serde::{Deserialize, Serialize};
use serde_json::Value;
fn event_version() -> u16 {
    1
}
pub const MAX_BATCH_BYTES: usize = 12 * 1024 * 1024;
pub const MAX_BATCH_EVENTS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JournalId(pub String);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventPosition(pub u64);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
/// Stable journal-global key; applications namespace IDs across principals and scopes.
pub struct ConsumerId(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StoredReceipt(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerEvent {
    #[serde(default = "event_version")]
    pub version: u16,
    pub position: EventPosition,
    pub created_at: i64,
    pub event_type: String,
    pub payload: Value,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerBatch {
    pub journal_id: JournalId,
    pub consumer_id: ConsumerId,
    /// Includes positions scanned but invisible to this service identity.
    pub start: EventPosition,
    pub end: EventPosition,
    pub events: Vec<BrokerEvent>,
    pub receipt: StoredReceipt,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerCheckpoint {
    pub journal_id: JournalId,
    pub consumer_id: ConsumerId,
    pub stored: EventPosition,
    pub floor: EventPosition,
    pub tail: EventPosition,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRequest {
    pub consumer_id: ConsumerId,
    pub identity: String,
    pub journal_id: Option<JournalId>,
    pub stored: EventPosition,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchRequest {
    pub consumer_id: ConsumerId,
    pub identity: String,
    pub max_events: usize,
    pub max_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckStoredRequest {
    pub consumer_id: ConsumerId,
    pub identity: String,
    pub journal_id: JournalId,
    pub end: EventPosition,
    pub receipt: StoredReceipt,
}

/// Explicit bootstrap policy for a previously unregistered consumer.
/// Existing consumers must resume their own durable checkpoint instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptionStart {
    /// Read every event retained after the journal's current pruning floor.
    EarliestAvailable,
    /// Begin after the current tail, intentionally skipping retained history.
    Latest,
}
