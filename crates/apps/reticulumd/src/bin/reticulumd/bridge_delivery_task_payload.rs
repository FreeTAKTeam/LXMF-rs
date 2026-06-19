use super::payload_builder::{build_outbound_payload, OutboundPayloadBuild};
use super::*;

impl DeliveryTask {
    pub(super) async fn build_payload(&self) -> Result<Vec<u8>, std::io::Error> {
        let stamp_work = self.normal_stamp_work();
        if let Some(work) = stamp_work {
            self.record_stamp_work_metadata("generating", work);
        }

        let result = build_outbound_payload(OutboundPayloadBuild {
            daemon: self.daemon.clone(),
            message_id: self.message_id.clone(),
            source_hash: self.source_hash,
            destination: self.destination,
            title: self.title.clone(),
            content: self.content.clone(),
            fields: self.fields.clone(),
            signer: self.signer.clone(),
            stamp_cost: self.stamp_cost,
            outbound_ticket: self.outbound_ticket.clone(),
            include_ticket: self.include_ticket.clone(),
        })
        .await;

        if let Some(work) = stamp_work {
            match &result {
                Ok(_) => self.record_stamp_work_metadata("ready", work),
                Err(err) => {
                    if Self::is_cancelled_status(
                        self.daemon
                            .message_receipt_status(&self.message_id)
                            .ok()
                            .flatten()
                            .as_deref(),
                    ) {
                        self.record_stamp_work_metadata("cancelled", work);
                    } else {
                        self.record_stamp_work_metadata("failed", work);
                        let _ = self.daemon.record_message_lxmf_metadata(
                            &self.message_id,
                            "stamp_error",
                            JsonValue::String(err.to_string()),
                        );
                    }
                }
            }
        }

        result
    }

    pub(super) fn requires_deferred_stamp_work(&self) -> bool {
        self.requires_normal_deferred_stamp_work()
            || self.requested_method == RequestedDeliveryMethod::Propagated
    }

    pub(super) fn requires_normal_deferred_stamp_work(&self) -> bool {
        self.normal_stamp_work().is_some()
    }

    pub(super) fn record_deferred_stamp_queued_metadata(&self) {
        if let Some(work) = self.normal_stamp_work() {
            let mut entries = self.stamp_work_entries("queued", work);
            entries.push(("stamp_attempts".to_string(), JsonValue::Number(0.into())));
            entries.push(("progress".to_string(), JsonValue::Number(0.into())));
            let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
        }
        if self.requested_method == RequestedDeliveryMethod::Propagated {
            let _ = self.daemon.record_message_lxmf_metadata_entries(
                &self.message_id,
                [
                    (
                        "propagation_stamp_state".to_string(),
                        JsonValue::String("queued".to_string()),
                    ),
                    ("propagation_stamp_attempts".to_string(), JsonValue::Number(0.into())),
                    ("progress".to_string(), JsonValue::Number(0.into())),
                    ("propagation_stamp_error".to_string(), JsonValue::Null),
                ],
            );
        }
    }

    pub(super) fn record_deferred_stamp_attempt_metadata(&self, attempt: u32) {
        if let Some(work) = self.normal_stamp_work() {
            let mut entries = self.stamp_work_entries("generating", work);
            entries.push((
                "stamp_attempts".to_string(),
                JsonValue::Number(serde_json::Number::from(attempt)),
            ));
            entries.push(("progress".to_string(), JsonValue::Number(0.into())));
            let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
        }
    }

    pub(super) fn record_deferred_stamp_retry_metadata(&self, attempt: u32, error: String) {
        if let Some(work) = self.normal_stamp_work() {
            let mut entries = self.stamp_work_entries("queued", work);
            entries.push((
                "stamp_attempts".to_string(),
                JsonValue::Number(serde_json::Number::from(attempt)),
            ));
            entries.push(("stamp_error".to_string(), JsonValue::String(error)));
            entries.push((
                "stamp_next_retry_at".to_string(),
                JsonValue::Number(serde_json::Number::from(now_secs_i64() + 1)),
            ));
            entries.push(("progress".to_string(), JsonValue::Number(0.into())));
            let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
        }
    }

    pub(super) fn record_deferred_stamp_failed_metadata(&self, attempt: u32, error: String) {
        if let Some(work) = self.normal_stamp_work() {
            let mut entries = self.stamp_work_entries("failed", work);
            entries.push((
                "stamp_attempts".to_string(),
                JsonValue::Number(serde_json::Number::from(attempt)),
            ));
            entries.push(("stamp_error".to_string(), JsonValue::String(error)));
            let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
        }
    }

    pub(super) fn record_deferred_stamp_cancelled_metadata(&self) {
        if let Some(work) = self.normal_stamp_work() {
            let mut entries = self.stamp_work_entries("cancelled", work);
            entries.push(("stamp_error".to_string(), JsonValue::Null));
            let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
        }
        if self.requested_method == RequestedDeliveryMethod::Propagated {
            let _ = self.daemon.record_message_lxmf_metadata_entries(
                &self.message_id,
                [
                    (
                        "propagation_stamp_state".to_string(),
                        JsonValue::String("cancelled".to_string()),
                    ),
                    ("propagation_stamp_error".to_string(), JsonValue::Null),
                ],
            );
        }
    }

    fn normal_stamp_work(&self) -> Option<StampWorkMetadata<'_>> {
        if let Some(ticket) = self.outbound_ticket.as_ref() {
            return Some(StampWorkMetadata {
                kind: "ticket",
                target_cost: Some(reticulum_daemon::lxmf_stamps::COST_TICKET),
                ticket: Some(ticket),
            });
        }
        self.stamp_cost.map(|cost| StampWorkMetadata {
            kind: "pow",
            target_cost: Some(cost),
            ticket: None,
        })
    }

    fn record_stamp_work_metadata(&self, state: &str, work: StampWorkMetadata<'_>) {
        let entries = self.stamp_work_entries(state, work);
        let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
    }

    fn stamp_work_entries(
        &self,
        state: &str,
        work: StampWorkMetadata<'_>,
    ) -> Vec<(String, JsonValue)> {
        let mut entries = vec![
            ("stamp_state".to_string(), JsonValue::String(state.to_string())),
            ("stamp_kind".to_string(), JsonValue::String(work.kind.to_string())),
        ];
        if let Some(target_cost) = work.target_cost {
            entries.push((
                "stamp_target_cost".to_string(),
                JsonValue::Number(serde_json::Number::from(target_cost)),
            ));
        }
        if let Some(ticket) = work.ticket {
            entries
                .push(("stamp_ticket_source".to_string(), JsonValue::String(ticket.to_string())));
        }
        if state != "failed" {
            entries.push(("stamp_error".to_string(), JsonValue::Null));
        }
        entries
    }
}

#[derive(Clone, Copy)]
struct StampWorkMetadata<'a> {
    kind: &'static str,
    target_cost: Option<u32>,
    ticket: Option<&'a str>,
}
