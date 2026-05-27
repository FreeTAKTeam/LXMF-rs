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
        let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
    }
}

#[derive(Clone, Copy)]
struct StampWorkMetadata<'a> {
    kind: &'static str,
    target_cost: Option<u32>,
    ticket: Option<&'a str>,
}
