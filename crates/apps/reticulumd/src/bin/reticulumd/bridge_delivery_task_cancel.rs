use super::*;

impl DeliveryTask {
    pub(super) fn abort_if_cancelled(&self, stage: &str) -> bool {
        self.abort_for_status_result(stage, self.daemon.message_receipt_status(&self.message_id))
    }

    pub(super) fn abort_for_status_result(
        &self,
        stage: &str,
        status: Result<Option<String>, std::io::Error>,
    ) -> bool {
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                log::error!(
                    "[daemon-delivery] receipt status lookup failed message_id={} stage={stage}: {error}",
                    self.message_id
                );
                let persisted = crate::receipt_events::persist_receipt_update(
                    self.daemon.as_ref(),
                    ReceiptEvent::new(
                        self.message_id.clone(),
                        format!("failed: receipt status lookup: {error}"),
                    )
                    .with_stage(stage),
                    &self.receipt_map,
                    &self.outbound_resource_map,
                );
                return match persisted {
                    Ok(()) => true,
                    Err(persist_error) => {
                        log::error!(
                            "[daemon-delivery] failed to persist receipt lookup failure message_id={} stage={stage}: {persist_error}; continuing because no terminal state was established",
                            self.message_id
                        );
                        false
                    }
                };
            }
        };
        if !Self::is_cancelled_status(status.as_deref()) {
            return false;
        }
        log_delivery_trace(&self.message_id, &self.destination_hex, stage, "cancelled");
        true
    }

    pub(super) fn is_cancelled_status(status: Option<&str>) -> bool {
        status.is_some_and(|value| value.trim().eq_ignore_ascii_case("cancelled"))
    }
}
