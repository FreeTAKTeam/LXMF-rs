use super::*;

impl DeliveryTask {
    pub(super) fn abort_if_cancelled(&self, stage: &str) -> bool {
        let status = match self.daemon.message_receipt_status(&self.message_id) {
            Ok(status) => status,
            Err(error) => {
                log::error!(
                    "[daemon-delivery] receipt status lookup failed message_id={} stage={stage}: {error}",
                    self.message_id
                );
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(
                        self.message_id.clone(),
                        format!("failed: receipt status lookup: {error}"),
                    )
                    .with_stage(stage),
                );
                return true;
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
