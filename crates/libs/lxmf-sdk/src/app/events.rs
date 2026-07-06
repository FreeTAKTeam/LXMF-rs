#[cfg(feature = "sdk-async")]
use crate::event::{EventBatch as RawEventBatch, EventSubscription, SdkEvent};
use crate::event::{Severity as RawSeverity, SubscriptionStart as RawSubscriptionStart};
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
pub struct StreamGapDetails {
    pub expected_seq_no: Option<u64>,
    pub observed_seq_no: Option<u64>,
    pub dropped_count: u64,
    pub recovery_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct InboundMessageDetails {
    pub message_id: Option<String>,
    pub source_hash: Option<String>,
    pub destination_hash: Option<String>,
    pub delivery_kind: Option<String>,
    pub lxmf_bytes_hex: Option<String>,
    pub receipt_status: Option<String>,
    pub signature_checked: Option<bool>,
    pub signature_status: Option<String>,
    pub stamp_status: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct InboundDropDetails {
    pub reason: Option<String>,
    pub delivery_kind: Option<String>,
    pub raw_destination_hash: Option<String>,
    pub resolved_destination_hash: Option<String>,
    pub source_hash: Option<String>,
    pub destination_hash: Option<String>,
    pub dropped_message_id: Option<String>,
    pub payload_mode: Option<String>,
    pub bytes_len: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeliveryLifecycleDetails {
    pub state: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub receipt_status: Option<String>,
    pub delivery_kind: Option<String>,
    pub packet_hash: Option<String>,
    pub resource_hash: Option<String>,
    pub peer: Option<String>,
    pub method: Option<String>,
    pub bytes: Option<u64>,
    pub link_id: Option<String>,
    pub reason: Option<String>,
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
            .then(|| inbound_message_details(&self.details))
    }
    pub fn inbound_drop_details(&self) -> Option<InboundDropDetails> {
        matches!(self.kind, EventKind::InboundMessageDropped)
            .then(|| inbound_drop_details(&self.details))
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
        .then(|| delivery_lifecycle_details(&self.details))
    }
}
fn json_str(value: &JsonValue, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

fn nested_json_str(value: &JsonValue, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(ToOwned::to_owned)
}

fn nested_json_bool(value: &JsonValue, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_bool()
}

fn inbound_message_details(payload: &JsonValue) -> InboundMessageDetails {
    let message = payload.get("message").unwrap_or(payload);
    InboundMessageDetails {
        message_id: json_str(message, "id").or_else(|| json_str(payload, "message_id")),
        source_hash: json_str(message, "source").or_else(|| json_str(payload, "source_hash")),
        destination_hash: json_str(message, "destination")
            .or_else(|| json_str(payload, "destination_hash")),
        delivery_kind: json_str(payload, "delivery_kind"),
        lxmf_bytes_hex: json_str(payload, "lxmf_bytes_hex"),
        receipt_status: json_str(message, "receipt_status")
            .or_else(|| json_str(payload, "receipt_status")),
        signature_checked: nested_json_bool(message, &["fields", "_lxmf", "signature_checked"]),
        signature_status: nested_json_str(message, &["fields", "_lxmf", "signature_status"]),
        stamp_status: nested_json_str(message, &["fields", "_lxmf", "stamp_status"]),
    }
}

fn inbound_drop_details(payload: &JsonValue) -> InboundDropDetails {
    InboundDropDetails {
        reason: json_str(payload, "reason"),
        delivery_kind: json_str(payload, "delivery_kind"),
        raw_destination_hash: json_str(payload, "raw_destination_hash"),
        resolved_destination_hash: json_str(payload, "resolved_destination_hash"),
        source_hash: json_str(payload, "source_hash"),
        destination_hash: json_str(payload, "destination_hash"),
        dropped_message_id: json_str(payload, "dropped_message_id"),
        payload_mode: json_str(payload, "payload_mode"),
        bytes_len: payload.get("bytes_len").and_then(JsonValue::as_u64),
        detail: json_str(payload, "detail"),
    }
}

fn delivery_lifecycle_details(payload: &JsonValue) -> DeliveryLifecycleDetails {
    let message = payload.get("message").unwrap_or(payload);
    DeliveryLifecycleDetails {
        state: json_str(payload, "state")
            .or_else(|| normalized_receipt_state(payload).ok().flatten()),
        from: json_str(payload, "from"),
        to: json_str(payload, "to"),
        receipt_status: json_str(message, "receipt_status")
            .or_else(|| json_str(payload, "receipt_status"))
            .or_else(|| json_str(payload, "status")),
        delivery_kind: json_str(payload, "delivery_kind"),
        packet_hash: json_str(payload, "packet_hash"),
        resource_hash: json_str(payload, "resource_hash"),
        peer: json_str(payload, "peer").or_else(|| json_str(payload, "peer_id")),
        method: json_str(payload, "method"),
        bytes: payload.get("bytes").and_then(JsonValue::as_u64),
        link_id: json_str(payload, "link_id"),
        reason: json_str(payload, "reason").or_else(|| json_str(payload, "detail")),
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

fn normalized_receipt_state(payload: &JsonValue) -> Result<Option<String>, &'static str> {
    let status_val = payload
        .get("message")
        .and_then(|message| message.get("receipt_status").or_else(|| message.get("status")))
        .or_else(|| payload.get("receipt_status"))
        .or_else(|| payload.get("status"))
        .or_else(|| payload.get("state"))
        .or_else(|| payload.get("receipt").and_then(|receipt| receipt.get("status")));
    let Some(status_val) = status_val else { return Ok(None) };
    let status = status_val.as_str().ok_or("receipt status is not a string")?;
    Ok(Some(status.split(':').next().unwrap_or(status).trim().to_ascii_lowercase()))
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
            normalized_receipt_state(&event.payload)
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
