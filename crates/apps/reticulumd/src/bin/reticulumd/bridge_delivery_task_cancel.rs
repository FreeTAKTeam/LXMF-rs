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
                    Ok(()) => {
                        self.receipt_tx.complete_terminal(&self.message_id);
                        true
                    }
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
        let durable = match self.daemon.owns_durable_message(&self.message_id) {
            Ok(value) => value,
            Err(error) => {
                log::error!("durable dispatch status unavailable: {error}; send suppressed");
                return true;
            }
        };
        let terminal = durable && status.as_deref().is_some_and(Self::is_terminal_status);
        if !terminal && !Self::is_cancelled_status(status.as_deref()) {
            return false;
        }
        self.receipt_tx.complete_terminal(&self.message_id);
        log_delivery_trace(&self.message_id, &self.destination_hex, stage, "cancelled");
        true
    }

    fn is_terminal_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        matches!(normalized.as_str(), "delivered" | "cancelled" | "expired" | "rejected")
            || normalized.starts_with("failed")
    }
    pub(super) fn is_cancelled_status(status: Option<&str>) -> bool {
        status.is_some_and(|value| value.trim().eq_ignore_ascii_case("cancelled"))
    }
}

#[cfg(test)]
mod durable_status_tests {
    use super::*;
    #[test]
    fn terminal_receipts_suppress_sends_with_storage_normalization() {
        for status in [" Delivered ", " FAILED: unreachable ", "cancelled", " EXPIRED ", "Rejected"]
        {
            assert!(DeliveryTask::is_terminal_status(status));
        }
        for status in ["sent: link", "sending", "queued"] {
            assert!(!DeliveryTask::is_terminal_status(status));
        }
    }
}
