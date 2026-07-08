#[cfg(feature = "sdk-async")]
use crate::event::{EventBatch as RawEventBatch, EventSubscription, SdkEvent};
use crate::event::{Severity as RawSeverity, SubscriptionStart as RawSubscriptionStart};
#[path = "events_details.rs"]
mod events_details;
pub use events_details::{
    DeliveryLifecycleDetails, InboundDropDetails, InboundMessageDetails, StreamGapDetails,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
    Critical,
    Unknown,
}

impl From<RawSeverity> for Severity {
    fn from(value: RawSeverity) -> Self {
        match value {
            RawSeverity::Debug => Self::Debug,
            RawSeverity::Info => Self::Info,
            RawSeverity::Warn => Self::Warn,
            RawSeverity::Error => Self::Error,
            RawSeverity::Critical => Self::Critical,
            RawSeverity::Unknown => Self::Unknown,
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

impl From<SubscriptionStart> for RawSubscriptionStart {
    fn from(value: SubscriptionStart) -> Self {
        match value {
            SubscriptionStart::Head => RawSubscriptionStart::Head,
            SubscriptionStart::Tail => RawSubscriptionStart::Tail,
            SubscriptionStart::Snapshot => RawSubscriptionStart::Snapshot,
        }
    }
}

impl From<RawSubscriptionStart> for SubscriptionStart {
    fn from(value: RawSubscriptionStart) -> Self {
        match value {
            RawSubscriptionStart::Head => Self::Head,
            RawSubscriptionStart::Tail => Self::Tail,
            RawSubscriptionStart::Snapshot => Self::Snapshot,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventMetadata {
    pub event_id: String,
    pub runtime_id: String,
    pub seq_no: u64,
    pub occurred_at_ms: u64,
    pub severity: Severity,
    pub operation_id: Option<String>,
    pub message_id: Option<String>,
    pub peer_id: Option<String>,
    pub correlation_id: Option<String>,
    pub profile_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum EventKind {
    RuntimeStarted,
    RuntimeStopped,
    RuntimeDegraded,
    RuntimeRecovered,
    AnnounceSent,
    AnnounceReceived,
    PeerDiscovered,
    PeerRemoved,
    ContactUpdated,
    ContactBootstrapped,
    CommandDispatched,
    CommandReceiptAcknowledged,
    CommandProcessingStarted,
    CommandProgress,
    CommandCompleted,
    CommandFailed,
    MessageQueued,
    MessageDispatching,
    MessageSent,
    MessageDelivered,
    MessageFailed,
    MessageCancelled,
    InboundMessageReceived,
    InboundMessageDropped,
    QueuePressureRaised,
    RetryScheduled,
    ReconnectScheduled,
    StreamGapDetected(StreamGapDetails),
    SecurityActionRequired,
    FatalErrorRaised,
    Unknown(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Event {
    pub metadata: EventMetadata,
    pub kind: EventKind,
    pub raw_event_type: String,
    pub details: JsonValue,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EventBatch {
    pub events: Vec<Event>,
    pub dropped_count: u64,
}

impl Event {
    pub fn inbound_message_details(&self) -> Option<InboundMessageDetails> {
        matches!(self.kind, EventKind::InboundMessageReceived)
            .then(|| events_details::inbound_message_details(&self.details))
    }
    pub fn inbound_drop_details(&self) -> Option<InboundDropDetails> {
        matches!(self.kind, EventKind::InboundMessageDropped)
            .then(|| events_details::inbound_drop_details(&self.details))
    }
    pub fn delivery_lifecycle_details(&self) -> Option<DeliveryLifecycleDetails> {
        matches!(
            self.kind,
            EventKind::MessageQueued
                | EventKind::MessageDispatching
                | EventKind::MessageSent
                | EventKind::MessageDelivered
                | EventKind::MessageFailed
                | EventKind::MessageCancelled
        )
        .then(|| events_details::delivery_lifecycle_details(&self.details))
    }
}

#[cfg(feature = "sdk-async")]
fn payload_state(payload: &JsonValue, key: &str) -> Result<Option<String>, &'static str> {
    match payload.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_str()
            .ok_or("payload field is not a string")
            .map(|s| Some(s.trim().to_ascii_lowercase())),
    }
}

#[cfg(feature = "sdk-async")]
fn map_delivery_state(state: &str) -> EventKind {
    match state {
        "queued" => EventKind::MessageQueued,
        "dispatching" | "sending" | "in_flight" => EventKind::MessageDispatching,
        "sent" => EventKind::MessageSent,
        "delivered" => EventKind::MessageDelivered,
        "failed" | "rejected" | "expired" => EventKind::MessageFailed,
        "cancelled" => EventKind::MessageCancelled,
        other => EventKind::Unknown(other.to_owned()),
    }
}

#[cfg(feature = "sdk-async")]
fn payload_peer_id(payload: &JsonValue) -> Result<Option<String>, &'static str> {
    for key in ["peer", "peer_id", "identity", "target", "source_hash", "destination_hash"] {
        match payload.get(key) {
            None => continue,
            Some(v) => {
                return v
                    .as_str()
                    .ok_or("peer id field is not a string")
                    .map(|s| Some(s.to_owned()));
            }
        }
    }
    if let Some(message) = payload.get("message") {
        for key in ["source", "source_hash", "destination", "destination_hash"] {
            match message.get(key) {
                None => continue,
                Some(v) => {
                    return v
                        .as_str()
                        .ok_or("message peer id field is not a string")
                        .map(|s| Some(s.to_owned()));
                }
            }
        }
    }
    Ok(None)
}

#[cfg(feature = "sdk-async")]
fn payload_message_id(payload: &JsonValue) -> Result<Option<String>, &'static str> {
    for key in ["message_id", "id", "dropped_message_id"] {
        match payload.get(key) {
            None => continue,
            Some(v) => {
                return v
                    .as_str()
                    .ok_or("message id field is not a string")
                    .map(|s| Some(s.to_owned()));
            }
        }
    }
    if let Some(message) = payload.get("message") {
        for key in ["id", "message_id"] {
            match message.get(key) {
                None => continue,
                Some(v) => {
                    return v
                        .as_str()
                        .ok_or("nested message id field is not a string")
                        .map(|s| Some(s.to_owned()));
                }
            }
        }
    }
    if let Some(receipt) = payload.get("receipt") {
        match receipt.get("message_id") {
            None => {}
            Some(v) => {
                return v
                    .as_str()
                    .ok_or("receipt message id field is not a string")
                    .map(|s| Some(s.to_owned()));
            }
        }
    }
    Ok(None)
}

#[cfg(feature = "sdk-async")]
pub fn map_sdk_event(event: SdkEvent, profile_id: &str) -> Event {
    let kind = match event.event_type.as_str() {
        "RuntimeStateChanged" => {
            let from = payload_state(&event.payload, "from").ok().flatten();
            let to = payload_state(&event.payload, "to").ok().flatten();
            match to.as_deref() {
                Some("running") if matches!(from.as_deref(), Some("failed")) => {
                    EventKind::RuntimeRecovered
                }
                Some("running") => EventKind::RuntimeStarted,
                Some("stopped") => EventKind::RuntimeStopped,
                Some("failed") => EventKind::FatalErrorRaised,
                Some("draining") => EventKind::RuntimeStopped,
                _ => EventKind::Unknown(event.event_type.clone()),
            }
        }
        "DeliveryStateTransition" => {
            let state = payload_state(&event.payload, "to")
                .ok()
                .flatten()
                .or_else(|| payload_state(&event.payload, "state").ok().flatten())
                .unwrap_or_else(|| "unknown".to_owned());
            map_delivery_state(state.as_str())
        }
        "DeliveryRetryScheduled" => EventKind::RetryScheduled,
        "RuntimeDegraded" | "runtime_degraded" => EventKind::RuntimeDegraded,
        "RuntimeRecovered" | "runtime_recovered" => EventKind::RuntimeRecovered,
        "ReconnectScheduled" | "reconnect_scheduled" => EventKind::ReconnectScheduled,
        "announce_sent" => EventKind::AnnounceSent,
        "announce_received" => EventKind::AnnounceReceived,
        "peer_sync" => EventKind::PeerDiscovered,
        "peer_unpeer" => EventKind::PeerRemoved,
        "contact_updated" => EventKind::ContactUpdated,
        "contact_bootstrapped" => EventKind::ContactBootstrapped,
        "command.dispatched" => EventKind::CommandDispatched,
        "command.receipt_acknowledged" => EventKind::CommandReceiptAcknowledged,
        "command.processing_started" => EventKind::CommandProcessingStarted,
        "command.progress" => EventKind::CommandProgress,
        "command.completed" => EventKind::CommandCompleted,
        "command.failed" => EventKind::CommandFailed,
        "InboundMessageReceived" | "inbound" => EventKind::InboundMessageReceived,
        "inbound_dropped" => EventKind::InboundMessageDropped,
        "StreamGap" => EventKind::StreamGapDetected(StreamGapDetails {
            expected_seq_no: event.payload.get("expected_seq_no").and_then(JsonValue::as_u64),
            observed_seq_no: event.payload.get("observed_seq_no").and_then(JsonValue::as_u64),
            dropped_count: event
                .payload
                .get("dropped_count")
                .and_then(JsonValue::as_u64)
                .unwrap_or_default(),
            recovery_required: true,
        }),
        "queue_pressure" | "store_forward_capacity_reached" | "store_forward_pruned" => {
            EventKind::QueuePressureRaised
        }
        "delivery_cancelled" => EventKind::MessageCancelled,
        "sdk_security_rate_limited" => EventKind::SecurityActionRequired,
        "runtime_shutdown_requested" => EventKind::RuntimeStopped,
        "outbound" | "receipt" => map_delivery_state(
            events_details::normalized_receipt_state(&event.payload)
                .ok()
                .flatten()
                .unwrap_or_else(|| "unknown".to_owned())
                .as_str(),
        ),
        "ErrorRaised" => {
            if matches!(event.severity, RawSeverity::Critical | RawSeverity::Error) {
                EventKind::FatalErrorRaised
            } else {
                EventKind::Unknown(event.event_type.clone())
            }
        }
        other => EventKind::Unknown(other.to_owned()),
    };

    Event {
        metadata: EventMetadata {
            event_id: event.event_id,
            runtime_id: event.runtime_id,
            seq_no: event.seq_no,
            occurred_at_ms: event.ts_ms,
            severity: event.severity.into(),
            operation_id: event.operation_id,
            message_id: event
                .message_id
                .or_else(|| payload_message_id(&event.payload).ok().flatten()),
            peer_id: event.peer_id.or_else(|| payload_peer_id(&event.payload).ok().flatten()),
            correlation_id: event.correlation_id,
            profile_id: profile_id.to_owned(),
        },
        kind,
        raw_event_type: event.event_type,
        details: event.payload,
        extensions: event.extensions,
    }
}

#[cfg(feature = "sdk-async")]
pub fn map_event_batch(batch: RawEventBatch, profile_id: &str) -> EventBatch {
    EventBatch {
        events: batch.events.into_iter().map(|event| map_sdk_event(event, profile_id)).collect(),
        dropped_count: batch.dropped_count,
    }
}
#[cfg(feature = "sdk-async")]
pub fn subscription_cursor(subscription: &EventSubscription) -> Option<crate::EventCursor> {
    subscription.cursor.clone()
}
#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
