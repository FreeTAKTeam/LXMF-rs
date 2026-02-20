use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendRequest {
    pub source: String,
    pub destination: String,
    pub payload: JsonValue,
    pub idempotency_key: Option<String>,
    pub ttl_ms: Option<u64>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl SendRequest {
    pub fn new(
        source: impl Into<String>,
        destination: impl Into<String>,
        payload: JsonValue,
    ) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            payload,
            idempotency_key: None,
            ttl_ms: None,
            correlation_id: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = Some(ttl_ms);
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

impl From<String> for MessageId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MessageId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Display for MessageId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeliveryState {
    Queued,
    Dispatching,
    InFlight,
    Sent,
    Delivered,
    Failed,
    Cancelled,
    Expired,
    Rejected,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct DeliverySnapshot {
    pub message_id: MessageId,
    pub state: DeliveryState,
    pub terminal: bool,
    pub last_updated_ms: u64,
    pub attempts: u32,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Ack {
    pub accepted: bool,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CancelResult {
    Accepted,
    AlreadyTerminal,
    NotFound,
    TooLateToCancel,
    Unsupported,
}
