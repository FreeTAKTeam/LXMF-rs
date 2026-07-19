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
                    let cancelled = match self.daemon.message_receipt_status(&self.message_id) {
                        Ok(status) => Self::is_cancelled_status(status.as_deref()),
                        Err(status_err) => {
                            log::warn!(
                                "[daemon] failed to read receipt status after payload build error message={} err={status_err}",
                                self.message_id
                            );
                            false
                        }
                    };
                    if cancelled {
                        self.record_stamp_work_metadata("cancelled", work);
                    } else {
                        self.record_stamp_work_metadata("failed", work);
                        self.record_lxmf_metadata(
                            "stamp_error",
                            JsonValue::String(err.to_string()),
                            "stamp_error",
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
            self.record_lxmf_metadata_entries(entries, "normal_stamp_queued");
        }
        if self.requested_method == RequestedDeliveryMethod::Propagated {
            self.record_lxmf_metadata_entries(
                [
                    (
                        "propagation_stamp_state".to_string(),
                        JsonValue::String("queued".to_string()),
                    ),
                    ("propagation_stamp_attempts".to_string(), JsonValue::Number(0.into())),
                    ("progress".to_string(), JsonValue::Number(0.into())),
                    ("propagation_stamp_error".to_string(), JsonValue::Null),
                ],
                "propagation_stamp_queued",
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
            self.record_lxmf_metadata_entries(entries, "normal_stamp_attempt");
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
            self.record_lxmf_metadata_entries(entries, "normal_stamp_retry");
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
            self.record_lxmf_metadata_entries(entries, "normal_stamp_failed");
        }
    }

    pub(super) fn record_deferred_stamp_cancelled_metadata(&self) {
        if let Some(work) = self.normal_stamp_work() {
            let mut entries = self.stamp_work_entries("cancelled", work);
            entries.push(("stamp_error".to_string(), JsonValue::Null));
            self.record_lxmf_metadata_entries(entries, "normal_stamp_cancelled");
        }
        if self.requested_method == RequestedDeliveryMethod::Propagated {
            self.record_lxmf_metadata_entries(
                [
                    (
                        "propagation_stamp_state".to_string(),
                        JsonValue::String("cancelled".to_string()),
                    ),
                    ("propagation_stamp_error".to_string(), JsonValue::Null),
                ],
                "propagation_stamp_cancelled",
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
        self.record_lxmf_metadata_entries(entries, "normal_stamp_state");
    }

    fn record_lxmf_metadata(&self, key: &str, value: JsonValue, context: &str) {
        if let Err(err) = self.daemon.record_message_lxmf_metadata(&self.message_id, key, value) {
            log::warn!(
                "[daemon] failed to record delivery metadata message={} context={context} err={err}",
                self.message_id
            );
        }
    }

    pub(super) fn record_lxmf_metadata_entries(
        &self,
        entries: impl IntoIterator<Item = (String, JsonValue)>,
        context: &str,
    ) {
        if let Err(err) =
            self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries)
        {
            log::warn!(
                "[daemon] failed to record delivery metadata message={} context={context} err={err}",
                self.message_id
            );
        }
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
