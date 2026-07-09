use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use lxmf_sdk::{
    DeliverySnapshot, DeliveryState, EventBatch, EventCursor, MessageId, RuntimeSnapshot,
    RuntimeState, SdkError, SdkEvent, Severity,
};
use serde_json::{json, Value as JsonValue};

use crate::config::InProcessBackendLimits;
use crate::delivery::InProcessSendReport;

pub(crate) struct BackendState {
    runtime_id: String,
    config_revision: u64,
    events: VecDeque<SdkEvent>,
    deliveries: HashMap<String, DeliverySnapshot>,
    send_reports: HashMap<String, InProcessSendReport>,
    send_report_order: VecDeque<String>,
    limits: InProcessBackendLimits,
}

impl BackendState {
    pub(crate) fn new(runtime_id: String, limits: InProcessBackendLimits) -> Self {
        Self {
            runtime_id,
            config_revision: 1,
            events: VecDeque::new(),
            deliveries: HashMap::new(),
            send_reports: HashMap::new(),
            send_report_order: VecDeque::new(),
            limits,
        }
    }

    pub(crate) fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub(crate) fn advance_config_revision(&mut self) -> u64 {
        self.config_revision = self.config_revision.saturating_add(1);
        self.config_revision
    }

    pub(crate) fn send_report(&self, message_id: &str) -> Option<InProcessSendReport> {
        self.send_reports.get(message_id).cloned()
    }

    pub(crate) fn record_send(&mut self, report: InProcessSendReport) -> Result<(), SdkError> {
        let message_id = report.message_id.0.clone();
        self.record_delivery(&message_id, DeliveryState::Sent, None)?;
        self.send_reports.insert(message_id.clone(), report.clone());
        self.send_report_order.retain(|item| item != &message_id);
        self.send_report_order.push_back(message_id.clone());
        while self.send_report_order.len() > self.limits.send_report_retention {
            if let Some(evicted) = self.send_report_order.pop_front() {
                self.send_reports.remove(&evicted);
            }
        }
        Ok(())
    }

    pub(crate) fn status(&self, id: &MessageId) -> Option<DeliverySnapshot> {
        self.deliveries.get(&id.0).cloned()
    }

    pub(crate) fn poll(
        &self,
        cursor: Option<&EventCursor>,
        max: usize,
    ) -> Result<EventBatch, SdkError> {
        let cursor_seq = cursor.and_then(|value| value.0.parse::<u64>().ok()).unwrap_or(0);
        let events = self
            .events
            .iter()
            .filter(|event| event.seq_no > cursor_seq)
            .take(max)
            .cloned()
            .collect::<Vec<_>>();
        let high_watermark = self.last_seq_no();
        let next_cursor = events.last().map(|event| event.seq_no).unwrap_or(high_watermark);
        serde_json::from_value(json!({
            "events": events,
            "next_cursor": next_cursor.to_string(),
            "dropped_count": 0,
            "snapshot_high_watermark_seq_no": high_watermark,
            "extensions": {},
        }))
        .map_err(|err| internal_error(format!("invalid event batch: {err}")))
    }

    pub(crate) fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        let in_flight = self.deliveries.values().filter(|item| !item.terminal).count() as u64;
        serde_json::from_value(json!({
            "runtime_id": self.runtime_id,
            "state": RuntimeState::Running,
            "active_contract_version": 2,
            "event_stream_position": self.last_seq_no(),
            "config_revision": self.config_revision,
            "queued_messages": 0,
            "in_flight_messages": in_flight,
        }))
        .map_err(|err| internal_error(format!("invalid runtime snapshot: {err}")))
    }

    pub(crate) fn record_delivery(
        &mut self,
        id: &str,
        state: DeliveryState,
        reason: Option<String>,
    ) -> Result<(), SdkError> {
        let attempts = self.deliveries.get(id).map_or(1, |item| item.attempts.saturating_add(1));
        let terminal = matches!(
            state,
            DeliveryState::Delivered
                | DeliveryState::Failed
                | DeliveryState::Cancelled
                | DeliveryState::Expired
                | DeliveryState::Rejected
                | DeliveryState::Unknown
        );
        let snapshot = serde_json::from_value(json!({
            "message_id": id,
            "state": state,
            "terminal": terminal,
            "last_updated_ms": now_ms(),
            "attempts": attempts,
            "reason_code": reason,
        }))
        .map_err(|err| internal_error(format!("invalid delivery snapshot: {err}")))?;
        self.deliveries.insert(id.to_owned(), snapshot);
        self.prune_deliveries();
        self.push_event(
            "lxmf.delivery.updated",
            if matches!(
                state,
                DeliveryState::Failed | DeliveryState::Rejected | DeliveryState::Expired
            ) {
                Severity::Warn
            } else {
                Severity::Info
            },
            json!({
                "message_id": id,
                "state": format!("{state:?}").to_ascii_lowercase(),
                "reason": reason,
            }),
        )
    }

    pub(crate) fn record_event(
        &mut self,
        event_type: &str,
        severity: Severity,
        payload: JsonValue,
    ) -> Result<(), SdkError> {
        self.push_event(event_type, severity, payload)
    }

    fn prune_deliveries(&mut self) {
        if self.deliveries.len() <= self.limits.delivery_retention {
            return;
        }
        let mut candidates = self
            .deliveries
            .iter()
            .map(|(id, item)| (id.clone(), !item.terminal, item.last_updated_ms))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, active, updated)| (*active, *updated));
        for (id, _, _) in candidates {
            if self.deliveries.len() <= self.limits.delivery_retention {
                break;
            }
            self.deliveries.remove(&id);
        }
    }

    fn push_event(
        &mut self,
        event_type: &str,
        severity: Severity,
        payload: JsonValue,
    ) -> Result<(), SdkError> {
        let seq_no = self.last_seq_no().saturating_add(1);
        let event = serde_json::from_value(json!({
            "event_id": format!("{}-{seq_no}", self.runtime_id),
            "runtime_id": self.runtime_id,
            "stream_id": "lxmf-runtime",
            "seq_no": seq_no,
            "contract_version": 2,
            "ts_ms": now_ms(),
            "event_type": event_type,
            "severity": severity,
            "source_component": "lxmf-runtime",
            "operation_id": null,
            "message_id": payload.get("message_id").and_then(JsonValue::as_str),
            "peer_id": payload.get("destination_hex").and_then(JsonValue::as_str),
            "correlation_id": null,
            "trace_id": null,
            "payload": payload,
            "extensions": {},
        }))
        .map_err(|err| internal_error(format!("invalid runtime event: {err}")))?;
        self.events.push_back(event);
        while self.events.len() > self.limits.event_retention {
            self.events.pop_front();
        }
        Ok(())
    }

    fn last_seq_no(&self) -> u64 {
        self.events.back().map_or(0, |event| event.seq_no)
    }
}

pub(crate) fn internal_error(message: impl Into<String>) -> SdkError {
    SdkError::new(lxmf_sdk::error_code::INTERNAL, lxmf_sdk::ErrorCategory::Internal, message)
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
