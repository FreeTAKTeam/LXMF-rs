use super::*;

impl DeliveryTask {
    pub(super) fn abort_if_cancelled(&self, stage: &str) -> bool {
        if !Self::is_cancelled_status(
            self.daemon.message_receipt_status(&self.message_id).ok().flatten().as_deref(),
        ) {
            return false;
        }
        log_delivery_trace(&self.message_id, &self.destination_hex, stage, "cancelled");
        true
    }

    pub(super) fn is_cancelled_status(status: Option<&str>) -> bool {
        status.is_some_and(|value| value.trim().eq_ignore_ascii_case("cancelled"))
    }
}
