use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventCursor(pub String);

impl From<String> for EventCursor {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SdkEvent {
    pub event_id: String,
    pub runtime_id: String,
    pub stream_id: String,
    pub seq_no: u64,
    pub contract_version: u16,
    pub ts_ms: u64,
    pub event_type: String,
    pub severity: Severity,
    pub source_component: String,
    pub operation_id: Option<String>,
    pub message_id: Option<String>,
    pub peer_id: Option<String>,
    pub correlation_id: Option<String>,
    pub trace_id: Option<String>,
    pub payload: JsonValue,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EventBatch {
    pub events: Vec<SdkEvent>,
    pub next_cursor: EventCursor,
    pub dropped_count: u64,
    pub snapshot_high_watermark_seq_no: Option<u64>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl EventBatch {
    pub fn empty(next_cursor: EventCursor) -> Self {
        Self {
            events: Vec::new(),
            next_cursor,
            dropped_count: 0,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SubscriptionStart {
    Head,
    Tail,
    Snapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventSubscription {
    pub start: SubscriptionStart,
    pub cursor: Option<EventCursor>,
}
