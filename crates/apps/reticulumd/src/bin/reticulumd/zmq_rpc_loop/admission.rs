use rns_rpc::rpc::zmq;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const CONTROL_COUNT: usize = 8;
const BULK_COUNT: usize = 24;
const CONTROL_BYTES: usize = 96 * 1024 * 1024;
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
            rejection: Lane::new(2, 32 * 1024),
        }
    }

    pub(super) fn rejection(&self) -> Option<Lease> {
        self.rejection.admit_with_reply(8192, 4096)
    }

    pub(super) fn admit(&self, encoded: &[u8]) -> std::io::Result<Option<Lease>> {
        let (method, retained) = zmq::request_admission(encoded)?;
        let lane = if control_method(method) { &self.control } else { &self.bulk };
        Ok(lane.admit_with_reply(retained, zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES))
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
