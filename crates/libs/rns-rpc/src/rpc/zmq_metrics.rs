//! Fixed-size ZeroMQ pipeline accounting. Byte counts cover owned wire buffers,
//! not decoded JSON, allocator metadata or socket-internal buffering.
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Instant;

const SLOW_STAGE_US: u64 = 250_000;
const WARNING_INTERVAL_US: u64 = 5_000_000;
const RECENT_FAILURE_CAPACITY: usize = 64;
const LATENCY_BUCKETS_US: [u64; 6] = [50_000, 100_000, 250_000, 500_000, 1_000_000, u64::MAX];
static WARNING_CLOCK: OnceLock<Instant> = OnceLock::new();

#[derive(Clone, Copy)]
pub enum ZmqStage {
    DispatchWait,
    Handler,
    ResponseQueue,
    Delivery,
    ResponseConnect,
    ResponseSend,
}

impl ZmqStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::DispatchWait => "dispatch_wait",
            Self::Handler => "handler",
            Self::ResponseQueue => "response_queue",
            Self::Delivery => "delivery",
            Self::ResponseConnect => "response_connect",
            Self::ResponseSend => "response_send",
        }
    }
}

#[derive(Clone, Copy)]
pub enum ZmqStageOutcome {
    Succeeded,
    Failed,
    TimedOut,
}

/// Small, identity-free request context retained only when a stage is slow or fails.
#[derive(Clone, Copy, Debug, Default)]
pub struct ZmqFailureContext {
    pub method: &'static str,
    pub queue_depth: usize,
    pub write_lock_wait_ns_total: Option<u64>,
    pub refill_in_flight: Option<bool>,
    pub announce_persistence_in_flight: Option<bool>,
    pub memory_events_high: Option<u64>,
}

impl ZmqFailureContext {
    pub fn bounded_method(method: &str) -> &'static str {
        match method {
            "sdk_poll_events_v2" => "sdk_poll_events_v2",
            "sdk_negotiate_v2" => "sdk_negotiate_v2",
            "sdk_send_v2" => "sdk_send_v2",
            "sdk_send_batch_v2" => "sdk_send_batch_v2",
            "sdk_cancel_message_v2" => "sdk_cancel_message_v2",
            "sdk_snapshot_v2" => "sdk_snapshot_v2",
            "sdk_status_v2" => "sdk_status_v2",
            "sdk_broker_negotiate_v1" => "sdk_broker_negotiate_v1",
            "sdk_broker_announces_v1" => "sdk_broker_announces_v1",
            "sdk_broker_resume_v1" => "sdk_broker_resume_v1",
            "sdk_broker_fetch_v1" => "sdk_broker_fetch_v1",
            "sdk_broker_ack_stored_v1" => "sdk_broker_ack_stored_v1",
            "sdk_broker_reconcile_v1" => "sdk_broker_reconcile_v1",
            "sdk_broker_admit_v1" => "sdk_broker_admit_v1",
            "status" => "status",
            "daemon_status_ex" => "daemon_status_ex",
            _ => "other",
        }
    }

    pub fn new(method: &str, queue_depth: usize) -> Self {
        Self { method: Self::bounded_method(method), queue_depth, ..Self::default() }
    }
}

#[derive(Default)]
struct LatencyBuckets([AtomicU64; 6]);

impl LatencyBuckets {
    fn observe(&self, elapsed_us: u64) {
        if let Some((index, _)) =
            LATENCY_BUCKETS_US.iter().enumerate().find(|(_, bound)| elapsed_us <= **bound)
        {
            self.0[index].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn snapshot(&self, count: u64, sum_us: u64) -> Value {
        let mut cumulative = 0;
        let mut buckets = serde_json::Map::new();
        for (bound, value) in LATENCY_BUCKETS_US.iter().zip(&self.0) {
            cumulative += value.load(Ordering::Relaxed);
            buckets.insert(
                if *bound == u64::MAX { "+Inf".to_owned() } else { bound.to_string() },
                json!(cumulative),
            );
        }
        json!({"buckets_us": buckets, "count": count, "sum_us": sum_us})
    }
}

#[derive(Default)]
struct StageCounters {
    active: AtomicU64,
    bytes: AtomicU64,
    peak_active: AtomicU64,
    peak_bytes: AtomicU64,
    succeeded: AtomicU64,
    failed: AtomicU64,
    timed_out: AtomicU64,
    cancelled: AtomicU64,
    elapsed_us: AtomicU64,
    max_elapsed_us: AtomicU64,
    slow: AtomicU64,
    suppressed_warnings: AtomicU64,
    last_warning_us: AtomicU64,
    latency_buckets: LatencyBuckets,
}

impl StageCounters {
    fn should_warn(&self, now_us: u64) -> bool {
        let tick = now_us.saturating_add(1); // zero means no previous warning
        let mut previous = self.last_warning_us.load(Ordering::Relaxed);
        loop {
            if previous != 0 && tick.saturating_sub(previous) < WARNING_INTERVAL_US {
                return false;
            }
            match self.last_warning_us.compare_exchange_weak(
                previous,
                tick,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(observed) => previous = observed,
            }
        }
    }
}

#[derive(Default)]
pub struct ZmqPipelineMetrics {
    stages: [Arc<StageCounters>; 6],
    last_delivery_failure: Mutex<Option<Value>>,
    recent_failures: Arc<Mutex<VecDeque<Value>>>,
}

impl ZmqPipelineMetrics {
    pub fn enter(&self, stage: ZmqStage, owned_wire_bytes: usize) -> ZmqStageGuard {
        let counters = Arc::clone(&self.stages[stage as usize]);
        let bytes = owned_wire_bytes as u64;
        let active = counters.active.fetch_add(1, Ordering::Relaxed) + 1;
        let total_bytes = counters.bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        counters.peak_active.fetch_max(active, Ordering::Relaxed);
        counters.peak_bytes.fetch_max(total_bytes, Ordering::Relaxed);
        ZmqStageGuard {
            counters,
            stage,
            bytes,
            started: Instant::now(),
            outcome: None,
            failure_context: ZmqFailureContext::new("other", 0),
            recent_failures: Arc::clone(&self.recent_failures),
        }
    }

    pub fn enter_correlated(
        &self,
        stage: ZmqStage,
        owned_wire_bytes: usize,
        context: ZmqFailureContext,
    ) -> ZmqStageGuard {
        let mut guard = self.enter(stage, owned_wire_bytes);
        guard.failure_context = context;
        guard
    }

    /// Retains one bounded, local correlation record. Successful polls do not
    /// erase a previous delivery failure; payloads, endpoints and auth are absent.
    pub fn record_delivery_failure(
        &self,
        session: &str,
        request_id: u64,
        stage: &'static str,
        timed_out: bool,
        elapsed_ms: u64,
    ) {
        let session: String = session
            .chars()
            .take(128)
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        *self.last_delivery_failure.lock().expect("ZMQ delivery diagnostics mutex poisoned") = Some(
            json!({
                "session_id": session, "request_id": request_id, "stage": stage,
                "error_code": if timed_out {"ZMQ_RESPONSE_DELIVERY_TIMEOUT"} else {"ZMQ_RESPONSE_DELIVERY_FAILED"},
                "elapsed_ms": elapsed_ms, "execution_certainty": "Unknown",
            }),
        );
    }

    pub fn snapshot(&self) -> Value {
        let mut result = serde_json::Map::new();
        result.insert(
            "last_delivery_failure".into(),
            self.last_delivery_failure
                .lock()
                .expect("ZMQ delivery diagnostics mutex poisoned")
                .clone()
                .unwrap_or(Value::Null),
        );
        result.insert(
            "recent_failures".into(),
            json!(self
                .recent_failures
                .lock()
                .expect("ZMQ failure ring mutex poisoned")
                .iter()
                .collect::<Vec<_>>()),
        );
        result.insert(
            "coverage".into(),
            json!("PUSH/PULL pipeline only; excludes canonical ROUTER/DEALER"),
        );
        result.insert("accounting".into(), json!("Per-stage wire accounting: ingress lengths, queued/active response capacities and encoded send-frame capacity; overlapping stages are not additive. Excludes decoded JSON, encode buffers during connect, route string clones, idle sockets, allocator metadata and socket buffers"));
        result.insert("consistency".into(), json!("Approximate independent atomic samples"));
        result.insert("slow_stage_us".into(), json!(SLOW_STAGE_US));
        result.insert("warning_interval_us".into(), json!(WARNING_INTERVAL_US));
        for stage in [
            ZmqStage::DispatchWait,
            ZmqStage::Handler,
            ZmqStage::ResponseQueue,
            ZmqStage::Delivery,
            ZmqStage::ResponseConnect,
            ZmqStage::ResponseSend,
        ] {
            let c = &self.stages[stage as usize];
            let read = |value: &AtomicU64| value.load(Ordering::Relaxed);
            let elapsed_us_total = read(&c.elapsed_us);
            let outcomes_total = read(&c.succeeded)
                .saturating_add(read(&c.failed))
                .saturating_add(read(&c.timed_out))
                .saturating_add(read(&c.cancelled));
            result.insert(stage.label().into(), json!({
                "active": read(&c.active), "owned_wire_bytes": read(&c.bytes),
                "peak_active": read(&c.peak_active), "peak_owned_wire_bytes": read(&c.peak_bytes),
                "succeeded": read(&c.succeeded), "failed": read(&c.failed),
                "timed_out": read(&c.timed_out), "cancelled": read(&c.cancelled),
                "elapsed_us_total": elapsed_us_total, "elapsed_us_max": read(&c.max_elapsed_us),
                "latency": c.latency_buckets.snapshot(outcomes_total, elapsed_us_total),
                "slow_total": read(&c.slow), "slow_warnings_suppressed_total": read(&c.suppressed_warnings),
            }));
        }
        Value::Object(result)
    }
}

/// Moves with the actual work/buffer. Drop accounts cancellation, including task
/// abort, failed channel send and shutdown queue disposal.
pub struct ZmqStageGuard {
    counters: Arc<StageCounters>,
    stage: ZmqStage,
    bytes: u64,
    started: Instant,
    outcome: Option<ZmqStageOutcome>,
    failure_context: ZmqFailureContext,
    recent_failures: Arc<Mutex<VecDeque<Value>>>,
}

impl ZmqStageGuard {
    pub fn finish(mut self, outcome: ZmqStageOutcome) {
        self.outcome = Some(outcome);
    }

    pub fn attach_failure_context(&mut self, context: ZmqFailureContext) {
        self.failure_context = context;
    }

    pub fn elapsed_us(&self) -> u64 {
        self.started.elapsed().as_micros().min(u64::MAX as u128) as u64
    }

    pub fn is_slow(&self) -> bool {
        self.elapsed_us() >= SLOW_STAGE_US
    }
}

impl Drop for ZmqStageGuard {
    fn drop(&mut self) {
        let c = &self.counters;
        c.active.fetch_sub(1, Ordering::Relaxed);
        c.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        let elapsed = self.elapsed_us();
        c.elapsed_us.fetch_add(elapsed, Ordering::Relaxed);
        c.max_elapsed_us.fetch_max(elapsed, Ordering::Relaxed);
        c.latency_buckets.observe(elapsed);
        let outcome = match self.outcome {
            Some(ZmqStageOutcome::Succeeded) => &c.succeeded,
            Some(ZmqStageOutcome::Failed) => &c.failed,
            Some(ZmqStageOutcome::TimedOut) => &c.timed_out,
            None => &c.cancelled,
        };
        outcome.fetch_add(1, Ordering::Relaxed);
        let outcome_name = match self.outcome {
            Some(ZmqStageOutcome::Succeeded) if elapsed >= SLOW_STAGE_US => Some("slow"),
            Some(ZmqStageOutcome::Succeeded) | None => None,
            Some(ZmqStageOutcome::Failed) => Some("failed"),
            Some(ZmqStageOutcome::TimedOut) => Some("timed_out"),
        };
        if let Some(outcome) = outcome_name {
            let mut ring = self.recent_failures.lock().expect("ZMQ failure ring mutex poisoned");
            if ring.len() == RECENT_FAILURE_CAPACITY {
                ring.pop_front();
            }
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
                .unwrap_or(0);
            ring.push_back(json!({
                "timestamp_unix_ms": timestamp_ms,
                "stage": self.stage.label(),
                "outcome": outcome,
                "method": self.failure_context.method,
                "queue_depth": self.failure_context.queue_depth,
                "write_lock_wait_ns_total": self.failure_context.write_lock_wait_ns_total,
                "refill_in_flight": self.failure_context.refill_in_flight,
                "announce_persistence_in_flight": self.failure_context.announce_persistence_in_flight,
                "memory_events_high": self.failure_context.memory_events_high,
            }));
        }
        if elapsed >= SLOW_STAGE_US {
            c.slow.fetch_add(1, Ordering::Relaxed);
            let now =
                WARNING_CLOCK.get_or_init(Instant::now).elapsed().as_micros().min(u64::MAX as u128)
                    as u64;
            if c.should_warn(now) {
                log::warn!(
                    "[daemon] zmq rpc slow stage={} elapsed_us={elapsed} owned_wire_bytes={}",
                    self.stage.label(),
                    self.bytes
                );
            } else {
                c.suppressed_warnings.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_failure_is_bounded_and_survives_successful_deliveries() {
        let metrics = ZmqPipelineMetrics::default();
        metrics.record_delivery_failure(
            &format!("secret\n雪{}", "x".repeat(1000)),
            42,
            "connect",
            true,
            1001,
        );
        metrics.enter(ZmqStage::Delivery, 0).finish(ZmqStageOutcome::Succeeded);
        let failure = metrics.snapshot()["last_delivery_failure"].clone();
        assert_eq!(failure["session_id"].as_str().expect("session").len(), 128);
        assert!(failure["session_id"].as_str().expect("session").is_ascii());
        assert_eq!(failure["request_id"], 42);
        assert_eq!(failure["error_code"], "ZMQ_RESPONSE_DELIVERY_TIMEOUT");
        assert_eq!(failure.as_object().expect("record").len(), 6);
    }

    #[test]
    fn slow_warning_rate_is_bounded_per_stage_without_a_growing_registry() {
        let counters = StageCounters::default();
        assert!(counters.should_warn(0));
        for tick in 1..10_000 {
            assert!(!counters.should_warn(tick));
        }
        assert!(!counters.should_warn(WARNING_INTERVAL_US - 1));
        assert!(counters.should_warn(WARNING_INTERVAL_US));
        assert!(!counters.should_warn(WARNING_INTERVAL_US));
    }

    #[test]
    fn stage_ownership_releases_on_completion_failure_timeout_and_drop() {
        let metrics = ZmqPipelineMetrics::default();
        for outcome in
            [ZmqStageOutcome::Succeeded, ZmqStageOutcome::Failed, ZmqStageOutcome::TimedOut]
        {
            metrics.enter(ZmqStage::Delivery, 4096).finish(outcome);
        }
        let first = metrics.enter(ZmqStage::Delivery, 100);
        let second = metrics.enter(ZmqStage::Delivery, 200);
        assert_eq!(metrics.snapshot()["delivery"]["owned_wire_bytes"], 300);
        drop((first, second));
        let snapshot = &metrics.snapshot()["delivery"];
        for key in ["active", "owned_wire_bytes"] {
            assert_eq!(snapshot[key], 0);
        }
        for key in ["succeeded", "failed", "timed_out"] {
            assert_eq!(snapshot[key], 1);
        }
        assert_eq!(snapshot["cancelled"], 2);
        assert_eq!(snapshot["peak_active"], 2);
        assert_eq!(snapshot["peak_owned_wire_bytes"], 4096);
    }

    #[test]
    fn incidents_keep_bounded_method_only_context_and_stage_latency_buckets() {
        let metrics = ZmqPipelineMetrics::default();
        let mut context = ZmqFailureContext::new("sdk_poll_events_v2", 7);
        context.write_lock_wait_ns_total = Some(123);
        context.memory_events_high = Some(9);
        for request in 0..RECENT_FAILURE_CAPACITY + 3 {
            let context = ZmqFailureContext { queue_depth: request, ..context };
            metrics
                .enter_correlated(ZmqStage::Handler, 0, context)
                .finish(ZmqStageOutcome::TimedOut);
        }
        let snapshot = metrics.snapshot();
        let failures = snapshot["recent_failures"].as_array().expect("failure ring");
        assert_eq!(failures.len(), RECENT_FAILURE_CAPACITY);
        let latest = failures.last().expect("latest failure");
        assert_eq!(latest["stage"], "handler");
        assert_eq!(latest["outcome"], "timed_out");
        assert_eq!(latest["method"], "sdk_poll_events_v2");
        assert_eq!(latest["queue_depth"], RECENT_FAILURE_CAPACITY + 2);
        assert_eq!(latest["write_lock_wait_ns_total"], 123);
        assert_eq!(latest["memory_events_high"], 9);
        assert_eq!(latest["refill_in_flight"], Value::Null);
        assert_eq!(latest["announce_persistence_in_flight"], Value::Null);
        assert!(latest.get("session_id").is_none());
        assert!(latest.get("request_id").is_none());
        assert_eq!(snapshot["handler"]["latency"]["count"], RECENT_FAILURE_CAPACITY as u64 + 3);
    }

    #[test]
    fn unrecognized_methods_collapse_to_other_in_failure_context() {
        assert_eq!(ZmqFailureContext::new("user supplied method", 0).method, "other");
        assert_eq!(ZmqFailureContext::new("sdk_broker_fetch_v1", 0).method, "sdk_broker_fetch_v1");
        assert_eq!(
            ZmqFailureContext::new("sdk_broker_ack_stored_v1", 0).method,
            "sdk_broker_ack_stored_v1"
        );
    }

    #[tokio::test]
    async fn slow_successful_stage_keeps_correlated_context() {
        let metrics = ZmqPipelineMetrics::default();
        let context = ZmqFailureContext::new("sdk_poll_events_v2", 3);
        let guard = metrics.enter_correlated(ZmqStage::Handler, 0, context);
        tokio::time::sleep(std::time::Duration::from_millis(260)).await;
        guard.finish(ZmqStageOutcome::Succeeded);

        let snapshot = metrics.snapshot();
        let failures = snapshot["recent_failures"].as_array().expect("recent stage records");
        let latest = failures.last().expect("slow stage record");
        assert_eq!(latest["stage"], "handler");
        assert_eq!(latest["outcome"], "slow");
        assert_eq!(latest["method"], "sdk_poll_events_v2");
        assert_eq!(latest["queue_depth"], 3);
    }

    #[tokio::test]
    async fn aborting_an_active_task_releases_its_stage() {
        let metrics = Arc::new(ZmqPipelineMetrics::default());
        let guard = metrics.enter(ZmqStage::ResponseQueue, 8192);
        let task = tokio::spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        task.abort();
        assert!(task.await.expect_err("cancelled").is_cancelled());
        assert_eq!(metrics.snapshot()["response_queue"]["active"], 0);
        assert_eq!(metrics.snapshot()["response_queue"]["owned_wire_bytes"], 0);
        assert_eq!(metrics.snapshot()["response_queue"]["cancelled"], 1);
    }
}
