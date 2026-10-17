use super::{validate_reservation_bytes, DeliveryScheduler};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::OwnedSemaphorePermit;

pub(in crate::bridge) struct DeliveryReservation {
    pub(super) count: OwnedSemaphorePermit,
    pub(super) bytes: OwnedSemaphorePermit,
    pub(super) ownership: DeliveryOwnership,
}

pub(super) struct DeliveryOwnership {
    messages: Arc<Mutex<HashSet<String>>>,
    message_id: String,
}

impl Drop for DeliveryOwnership {
    fn drop(&mut self) {
        self.messages.lock().expect("delivery ownership mutex poisoned").remove(&self.message_id);
    }
}

impl DeliveryScheduler {
    /// Claim the message before bridge preparation. Duplicate admission is a
    /// successful no-op even when the existing owner occupies all capacity.
    pub(in crate::bridge) fn reserve_message(
        &self,
        message_id: &str,
        bytes: usize,
    ) -> Result<Option<DeliveryReservation>, std::io::Error> {
        let mut messages = self.owned_messages.lock().expect("delivery ownership mutex poisoned");
        if messages.contains(message_id) {
            return Ok(None);
        }
        validate_reservation_bytes(bytes)?;
        if *self.owner.stop.borrow() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "delivery runtime stopping",
            ));
        }
        let count = self.backlog_limit.clone().try_acquire_owned().map_err(|_| {
            self.metrics.record_queue_full();
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "outbound delivery count capacity exhausted",
            )
        })?;
        let bytes = u32::try_from(bytes).map_err(std::io::Error::other)?;
        let bytes = self.bytes.clone().try_acquire_many_owned(bytes).map_err(|_| {
            self.metrics.record_queue_full();
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "outbound delivery byte capacity exhausted",
            )
        })?;
        messages.insert(message_id.to_owned());
        Ok(Some(DeliveryReservation {
            count,
            bytes,
            ownership: DeliveryOwnership {
                messages: self.owned_messages.clone(),
                message_id: message_id.to_owned(),
            },
        }))
    }
}
