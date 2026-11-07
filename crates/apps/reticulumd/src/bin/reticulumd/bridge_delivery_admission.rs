use super::{validate_reservation_bytes, DeliveryScheduler};
use crate::bridge::delivery_wakeup::DeliveryWakeup;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::OwnedSemaphorePermit;

pub(in crate::bridge) struct DeliveryReservation {
    pub(super) count: OwnedSemaphorePermit,
    pub(super) bytes: OwnedSemaphorePermit,
    pub(super) ownership: DeliveryOwnership,
}

pub(super) struct DeliveryOwnership {
    messages: Arc<Mutex<HashMap<String, Arc<DeliveryWakeup>>>>,
    message_id: String,
    pub(super) wakeup: Arc<DeliveryWakeup>,
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
        if messages.contains_key(message_id) {
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
        let wakeup = Arc::new(DeliveryWakeup::default());
        messages.insert(message_id.to_owned(), wakeup.clone());
        Ok(Some(DeliveryReservation {
            count,
            bytes,
            ownership: DeliveryOwnership {
                messages: self.owned_messages.clone(),
                message_id: message_id.to_owned(),
                wakeup,
            },
        }))
    }
}

impl DeliveryScheduler {
    pub(in crate::bridge) fn owns_delivery(&self, id: &str) -> bool {
        self.owned_messages.lock().expect("delivery ownership mutex poisoned").contains_key(id)
    }

    pub(in crate::bridge) fn notify_path_available(&self, destination: &str) {
        let hash = match rns_core::destination_hash::parse_destination_hash_required(destination) {
            Ok(hash) => rns_transport::hash::AddressHash::new(hash),
            Err(error) => {
                log::warn!("invalid delivery wake destination={destination}: {error}");
                return;
            }
        };
        for wakeup in
            self.owned_messages.lock().expect("delivery ownership mutex poisoned").values()
        {
            wakeup.notify_path(hash);
        }
    }
}
