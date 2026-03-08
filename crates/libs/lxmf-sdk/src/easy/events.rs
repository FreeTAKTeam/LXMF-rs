use crate::event::{EventBatch, EventSubscription, SdkEvent, Severity, SubscriptionStart};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EasySeverity {
    Debug,
    Info,
    Warn,
    Error,
    Critical,
    Unknown,
}

impl From<Severity> for EasySeverity {
    fn from(value: Severity) -> Self {
        match value {
            Severity::Debug => Self::Debug,
            Severity::Info => Self::Info,
            Severity::Warn => Self::Warn,
            Severity::Error => Self::Error,
            Severity::Critical => Self::Critical,
            Severity::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EasySubscriptionStart {
    Head,
    Tail,
    Snapshot,
}

impl From<EasySubscriptionStart> for SubscriptionStart {
    fn from(value: EasySubscriptionStart) -> Self {
        match value {
            EasySubscriptionStart::Head => SubscriptionStart::Head,
            EasySubscriptionStart::Tail => SubscriptionStart::Tail,
            EasySubscriptionStart::Snapshot => SubscriptionStart::Snapshot,
        }
    }
}

impl From<SubscriptionStart> for EasySubscriptionStart {
    fn from(value: SubscriptionStart) -> Self {
        match value {
            SubscriptionStart::Head => Self::Head,
            SubscriptionStart::Tail => Self::Tail,
            SubscriptionStart::Snapshot => Self::Snapshot,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct EasyStreamGapDetails {
    pub expected_seq_no: Option<u64>,
    pub observed_seq_no: Option<u64>,
    pub dropped_count: u64,
    pub recovery_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct EasyEventMetadata {
    pub event_id: String,
    pub runtime_id: String,
    pub seq_no: u64,
    pub occurred_at_ms: u64,
    pub severity: EasySeverity,
    pub operation_id: Option<String>,
    pub message_id: Option<String>,
    pub correlation_id: Option<String>,
    pub profile_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum EasyEventKind {
    RuntimeStarted,
    RuntimeStopped,
    RuntimeDegraded,
    RuntimeRecovered,
    MessageQueued,
    MessageDispatching,
    MessageSent,
    MessageDelivered,
    MessageFailed,
    MessageCancelled,
    InboundMessageReceived,
    QueuePressureRaised,
    RetryScheduled,
    ReconnectScheduled,
    StreamGapDetected(EasyStreamGapDetails),
    SecurityActionRequired,
    FatalErrorRaised,
    Unknown(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EasyEvent {
    pub metadata: EasyEventMetadata,
    pub kind: EasyEventKind,
    pub raw_event_type: String,
    pub details: JsonValue,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EasyEventBatch {
    pub events: Vec<EasyEvent>,
    pub dropped_count: u64,
}

fn payload_state(payload: &JsonValue, key: &str) -> Option<String> {
    payload.get(key).and_then(JsonValue::as_str).map(|value| value.trim().to_ascii_lowercase())
}

fn receipt_state(payload: &JsonValue) -> Option<String> {
    let status = payload
        .get("message")
        .and_then(|message| message.get("receipt_status"))
        .and_then(JsonValue::as_str)?;
    Some(status.split(':').next().unwrap_or(status).trim().to_ascii_lowercase())
}

fn map_delivery_state(state: &str) -> EasyEventKind {
    match state {
        "queued" => EasyEventKind::MessageQueued,
        "dispatching" | "sending" | "in_flight" => EasyEventKind::MessageDispatching,
        "sent" => EasyEventKind::MessageSent,
        "delivered" => EasyEventKind::MessageDelivered,
        "failed" | "rejected" | "expired" => EasyEventKind::MessageFailed,
        "cancelled" => EasyEventKind::MessageCancelled,
        other => EasyEventKind::Unknown(other.to_owned()),
    }
}

pub fn map_sdk_event(event: SdkEvent, profile_id: &str) -> EasyEvent {
    let kind = match event.event_type.as_str() {
        "RuntimeStateChanged" => {
            let from = payload_state(&event.payload, "from");
            let to = payload_state(&event.payload, "to");
            match to.as_deref() {
                Some("running") if matches!(from.as_deref(), Some("failed")) => {
                    EasyEventKind::RuntimeRecovered
                }
                Some("running") => EasyEventKind::RuntimeStarted,
                Some("stopped") => EasyEventKind::RuntimeStopped,
                Some("failed") => EasyEventKind::FatalErrorRaised,
                Some("draining") => EasyEventKind::RuntimeStopped,
                _ => EasyEventKind::Unknown(event.event_type.clone()),
            }
        }
        "DeliveryStateTransition" => {
            let state = payload_state(&event.payload, "to")
                .or_else(|| payload_state(&event.payload, "state"))
                .unwrap_or_else(|| "unknown".to_owned());
            map_delivery_state(state.as_str())
        }
        "DeliveryRetryScheduled" => EasyEventKind::RetryScheduled,
        "InboundMessageReceived" | "inbound" => EasyEventKind::InboundMessageReceived,
        "StreamGap" => EasyEventKind::StreamGapDetected(EasyStreamGapDetails {
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
            EasyEventKind::QueuePressureRaised
        }
        "delivery_cancelled" => EasyEventKind::MessageCancelled,
        "sdk_security_rate_limited" => EasyEventKind::SecurityActionRequired,
        "runtime_shutdown_requested" => EasyEventKind::RuntimeStopped,
        "outbound" => map_delivery_state(
            receipt_state(&event.payload).unwrap_or_else(|| "unknown".to_owned()).as_str(),
        ),
        "ErrorRaised" => {
            if matches!(event.severity, Severity::Critical | Severity::Error) {
                EasyEventKind::FatalErrorRaised
            } else {
                EasyEventKind::Unknown(event.event_type.clone())
            }
        }
        other => EasyEventKind::Unknown(other.to_owned()),
    };

    EasyEvent {
        metadata: EasyEventMetadata {
            event_id: event.event_id,
            runtime_id: event.runtime_id,
            seq_no: event.seq_no,
            occurred_at_ms: event.ts_ms,
            severity: event.severity.into(),
            operation_id: event.operation_id,
            message_id: event.message_id,
            correlation_id: event.correlation_id,
            profile_id: profile_id.to_owned(),
        },
        kind,
        raw_event_type: event.event_type,
        details: event.payload,
        extensions: event.extensions,
    }
}

pub fn map_event_batch(batch: EventBatch, profile_id: &str) -> EasyEventBatch {
    EasyEventBatch {
        events: batch.events.into_iter().map(|event| map_sdk_event(event, profile_id)).collect(),
        dropped_count: batch.dropped_count,
    }
}

pub fn subscription_cursor(subscription: &EventSubscription) -> Option<crate::EventCursor> {
    subscription.cursor.clone()
}

#[cfg(test)]
mod tests {
    use super::{map_sdk_event, EasyEventKind, EasySubscriptionStart};
    use crate::{SdkEvent, Severity, SubscriptionStart};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn base_event(event_type: &str, payload: serde_json::Value) -> SdkEvent {
        SdkEvent {
            event_id: "evt-1".to_owned(),
            runtime_id: "rt-1".to_owned(),
            stream_id: "stream-1".to_owned(),
            seq_no: 1,
            contract_version: 2,
            ts_ms: 10,
            event_type: event_type.to_owned(),
            severity: Severity::Info,
            source_component: "test".to_owned(),
            operation_id: None,
            message_id: None,
            peer_id: None,
            correlation_id: None,
            trace_id: None,
            payload,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn maps_runtime_state_change_to_started() {
        let mapped = map_sdk_event(
            base_event("RuntimeStateChanged", json!({ "from": "starting", "to": "running" })),
            "desktop_default",
        );
        assert!(matches!(mapped.kind, EasyEventKind::RuntimeStarted));
    }

    #[test]
    fn maps_stream_gap_to_typed_gap_event() {
        let mapped = map_sdk_event(
            base_event(
                "StreamGap",
                json!({ "expected_seq_no": 2, "observed_seq_no": 7, "dropped_count": 5 }),
            ),
            "desktop_default",
        );
        match mapped.kind {
            EasyEventKind::StreamGapDetected(details) => {
                assert_eq!(details.expected_seq_no, Some(2));
                assert_eq!(details.observed_seq_no, Some(7));
                assert_eq!(details.dropped_count, 5);
                assert!(details.recovery_required);
            }
            other => panic!("expected stream gap event, got {other:?}"),
        }
    }

    #[test]
    fn easy_subscription_start_round_trips() {
        let raw: SubscriptionStart = EasySubscriptionStart::Tail.into();
        assert_eq!(EasySubscriptionStart::from(raw), EasySubscriptionStart::Tail);
    }
}
