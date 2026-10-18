use rns_transport::hash::AddressHash;
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Default)]
pub(super) struct DeliveryWakeup {
    target: Mutex<Option<AddressHash>>,
    #[cfg(test)]
    pub(in crate::bridge) trace: Mutex<Option<std::sync::Arc<trace_control::TraceControl>>>,
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

#[cfg(test)]
#[path = "bridge_delivery_trace_control.rs"]
pub(in crate::bridge) mod trace_control;

#[cfg(test)]
impl DeliveryWakeup {
    pub(in crate::bridge) fn trace(&self) -> Option<std::sync::Arc<trace_control::TraceControl>> {
        self.trace.lock().expect("trace control").clone()
    }
}
