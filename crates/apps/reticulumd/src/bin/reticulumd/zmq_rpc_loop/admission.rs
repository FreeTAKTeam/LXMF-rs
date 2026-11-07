use rns_rpc::rpc::zmq;
use rns_rpc::rpc::zmq_metrics::ZmqFailureContext;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const CONTROL_COUNT: usize = 8;
const BULK_COUNT: usize = 24;
// Configurable poll batches can fill a legal envelope; reserve that actual ceiling.
const CONTROL_BYTES: usize = CONTROL_COUNT * 2 * zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES;
pub(super) const REJECTION_COUNT: usize = 32;
const BULK_BYTES: usize = 160 * 1024 * 1024;

pub(super) struct Admission {
    control: Lane,
    bulk: Lane,
    rejection: Lane,
}

struct Lane {
    count: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

/// Admission remains owned through response delivery, including queued and
/// unpolled work. Reserve a legal maximum reply before dispatch can allocate it.
pub(super) struct Lease {
    _count: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

impl Lane {
    fn new(count: usize, bytes: usize) -> Self {
        Self { count: Arc::new(Semaphore::new(count)), bytes: Arc::new(Semaphore::new(bytes)) }
    }

    fn admit_with_reply(&self, input_bytes: usize, reply_bytes: usize) -> Option<Lease> {
        let required = input_bytes.checked_add(reply_bytes)?;
        Some(Lease {
            _count: Arc::clone(&self.count).try_acquire_owned().ok()?,
            _bytes: Arc::clone(&self.bytes)
                .try_acquire_many_owned(u32::try_from(required).ok()?)
                .ok()?,
        })
    }
}

impl Admission {
    pub(super) fn new() -> Self {
        Self {
            control: Lane::new(CONTROL_COUNT, CONTROL_BYTES),
            bulk: Lane::new(BULK_COUNT, BULK_BYTES),
            rejection: Lane::new(REJECTION_COUNT, REJECTION_COUNT * 12 * 1024),
        }
    }

    pub(super) fn warn_rejection_drop(&self) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static DROPS: AtomicU64 = AtomicU64::new(0);
        let dropped = DROPS.fetch_add(1, Ordering::Relaxed) + 1;
        if dropped.is_power_of_two() {
            log::warn!("[daemon] overload reply lane full; dropped_total={dropped}");
        }
    }

    pub(super) fn rejection(&self) -> Option<Lease> {
        self.rejection.admit_with_reply(8192, 4096)
    }

    pub(super) fn admit(&self, encoded: &[u8]) -> std::io::Result<(Option<Lease>, &'static str)> {
        let (method, retained) = zmq::request_admission(encoded)?;
        let lane = if control_method(method) { &self.control } else { &self.bulk };
        let bounded_method = ZmqFailureContext::bounded_method(method);
        Ok((lane.admit_with_reply(retained, zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES), bounded_method))
    }
}

fn control_method(method: &str) -> bool {
    matches!(
        method,
        "sdk_poll_events_v2"
            | "sdk_snapshot_v2"
            | "sdk_status_v2"
            | "sdk_cursor_hint_v2"
            | "sdk_broker_negotiate_v1"
            | "sdk_broker_announces_v1"
            | "sdk_broker_resume_v1"
            | "sdk_broker_fetch_v1"
            | "sdk_broker_ack_stored_v1"
            | "sdk_broker_reconcile_v1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eight_control_replies_fit_and_rejections_have_separate_capacity() {
        let admission = Admission::new();
        let leases: Vec<_> = (0..CONTROL_COUNT)
            .map(|_| {
                admission
                    .control
                    .admit_with_reply(
                        zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES,
                        zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES,
                    )
                    .expect("control slot with a maximum legal reply")
            })
            .collect();
        assert!(admission.control.admit_with_reply(1, 1).is_none());
        let rejections: Vec<_> = (0..REJECTION_COUNT)
            .map(|_| admission.rejection().expect("bounded busy reply"))
            .collect();
        assert!(admission.rejection().is_none());
        drop((leases, rejections));
    }

    #[test]
    fn bulk_cannot_consume_reserved_control_reply_capacity() {
        let admission = Admission::new();
        let mut held = Vec::new();
        while let Some(lease) =
            admission.bulk.admit_with_reply(4096, zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES)
        {
            held.push(lease);
        }
        assert!(!held.is_empty());
        assert!(held.len() <= BULK_COUNT);
        assert!(admission
            .control
            .admit_with_reply(4096, zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES)
            .is_some());
        assert!(admission
            .control
            .admit_with_reply(zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES, zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES)
            .is_some());
        drop(held);
        assert!(admission.bulk.admit_with_reply(4096, zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES).is_some());
    }
}
