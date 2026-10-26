use super::RpcDaemon;
use std::sync::atomic::{AtomicUsize, Ordering};

struct BoundListener<'a>(&'a AtomicUsize);
impl Drop for BoundListener<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RpcDaemon {
    /// Configure once before starting any listener or HTTP server.
    pub fn configure_zmq_readiness(&self, expected: usize) {
        self.zmq_expected_listeners.store(expected, Ordering::Release);
    }

    /// The loop owns this guard from successful bind through exit/cancellation.
    pub fn zmq_listener_bound(&self) -> impl Drop + '_ {
        self.zmq_bound_listeners.fetch_add(1, Ordering::AcqRel);
        BoundListener(&self.zmq_bound_listeners)
    }

    pub fn is_ready(&self) -> bool {
        self.zmq_bound_listeners.load(Ordering::Acquire)
            >= self.zmq_expected_listeners.load(Ordering::Acquire)
    }
}
