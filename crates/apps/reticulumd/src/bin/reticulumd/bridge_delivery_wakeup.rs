use rns_transport::hash::AddressHash;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct DeliveryWakeup {
    target: Mutex<Option<AddressHash>>,
    pub(super) changed: Notify,
}

impl DeliveryWakeup {
    pub(super) fn wait_for(&self, target: AddressHash) {
        *self.target.lock().expect("delivery wait target poisoned") = Some(target);
    }

    pub(super) fn notify_path(&self, target: AddressHash) {
        if *self.target.lock().expect("delivery wait target poisoned") == Some(target) {
            self.changed.notify_one();
        }
    }
}
