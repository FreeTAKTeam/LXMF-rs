use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use lxmf_sdk::app::{
    Client as AppClient, Config as AppConfig, SubscriptionStart as AppSubscriptionStart,
};
use lxmf_sdk::{
    required_capabilities, Ack, AuthMode, BindMode, CancelResult, Client, ConfigPatch,
    DeliverySnapshot, DeliveryState, EffectiveLimits, EventBatch, EventCursor, EventStreamConfig,
    EventSubscription, LxmfSdk, MessageId, NegotiationRequest, NegotiationResponse, OverflowPolicy,
    Profile, RedactionConfig, RedactionTransform, RuntimeSnapshot, RuntimeState, SdkBackend,
    SdkBackendAsyncEvents, SdkBackendAsyncOps, SdkConfig, SdkError, SdkEvent, SdkEventStream,
    SendRequest, Severity, ShutdownMode, StartRequest, SubscriptionStart,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_stream::StreamExt;

#[derive(Default)]
struct BenchBackend {
    next_id: AtomicU64,
}

struct StreamBenchBackend {
    live_events: Mutex<VecDeque<SdkEvent>>,
    catchup_events: Mutex<VecDeque<SdkEvent>>,
}

impl StreamBenchBackend {
    fn fanout(event_count: u64) -> Self {
        Self {
            live_events: Mutex::new((1..=event_count).map(bench_sdk_event).collect()),
            catchup_events: Mutex::new(VecDeque::new()),
        }
    }

    fn reconnect_catchup(live_event_count: u64, catchup_event_count: u64) -> Self {
        Self {
            live_events: Mutex::new((1..=live_event_count).map(bench_sdk_event).collect()),
            catchup_events: Mutex::new(
                ((live_event_count + 1)..=(live_event_count + catchup_event_count))
                    .map(bench_sdk_event)
                    .collect(),
            ),
        }
    }
}

#[derive(Default)]
struct SlowSubscriberStats {
    queued: AtomicUsize,
    producer_pending: AtomicBool,
    peak_buffered: AtomicUsize,
}

impl SlowSubscriberStats {
    fn observe(&self) {
        let buffered = self.queued.load(Ordering::Relaxed)
            + usize::from(self.producer_pending.load(Ordering::Relaxed));
        let mut current = self.peak_buffered.load(Ordering::Relaxed);
        while buffered > current {
            match self.peak_buffered.compare_exchange_weak(
                current,
                buffered,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }
}

struct SlowSubscriberBenchBackend {
    event_count: u64,
    stats: Arc<SlowSubscriberStats>,
}

impl SlowSubscriberBenchBackend {
    fn new(event_count: u64, stats: Arc<SlowSubscriberStats>) -> Self {
        Self { event_count, stats }
    }
}

impl SdkBackend for BenchBackend {
    fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let mut effective_capabilities = required_capabilities(Profile::DesktopFull)
            .iter()
            .map(|capability| (*capability).to_string())
            .collect::<Vec<_>>();
        effective_capabilities.push("sdk.capability.cursor_replay".to_string());
        effective_capabilities.push("sdk.capability.async_events".to_string());
        effective_capabilities.sort();
        effective_capabilities.dedup();
        let effective_limits = from_json::<EffectiveLimits>(json!({
            "max_poll_events": 256,
            "max_event_bytes": 65_536,
            "max_batch_bytes": 1_048_576,
            "max_extension_keys": 32,
            "idempotency_ttl_ms": 86_400_000
        }));
        Ok(from_json::<NegotiationResponse>(json!({
            "runtime_id": "bench-runtime",
            "active_contract_version": 2,
            "effective_capabilities": effective_capabilities,
            "effective_limits": effective_limits,
            "contract_release": "v2.5",
            "schema_namespace": "v2"
        })))
    }

    fn send(&self, _req: SendRequest) -> Result<MessageId, SdkError> {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        Ok(MessageId(format!("bench-msg-{seq}")))
    }

    fn cancel(&self, _id: MessageId) -> Result<CancelResult, SdkError> {
        Ok(CancelResult::Accepted)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        Ok(Some(from_json::<DeliverySnapshot>(json!({
            "message_id": id,
            "state": DeliveryState::Sent,
            "terminal": true,
            "last_updated_ms": 0,
            "attempts": 1,
            "reason_code": null
        }))))
    }

    fn configure(&self, _expected_revision: u64, _patch: ConfigPatch) -> Result<Ack, SdkError> {
        Ok(from_json::<Ack>(json!({ "accepted": true, "revision": 1 })))
    }

    fn poll_events(
        &self,
        _cursor: Option<EventCursor>,
        _max: usize,
    ) -> Result<EventBatch, SdkError> {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        Ok(from_json::<EventBatch>(json!({
            "events": [bench_sdk_event(seq)],
            "next_cursor": format!("bench-cursor-{seq}"),
            "dropped_count": 0,
            "snapshot_high_watermark_seq_no": null,
            "extensions": {}
        })))
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        Ok(from_json::<RuntimeSnapshot>(json!({
            "runtime_id": "bench-runtime",
            "state": RuntimeState::Running,
            "active_contract_version": 2,
            "event_stream_position": 0,
            "config_revision": 0,
            "queued_messages": 0,
            "in_flight_messages": 0
        })))
    }

    fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
        Ok(from_json::<Ack>(json!({ "accepted": true, "revision": null })))
    }
}

impl SdkBackend for StreamBenchBackend {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        BenchBackend::default().negotiate(req)
    }

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        BenchBackend::default().send(req)
    }

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        BenchBackend::default().cancel(id)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        BenchBackend::default().status(id)
    }

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError> {
        BenchBackend::default().configure(expected_revision, patch)
    }

    fn poll_events(
        &self,
        _cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<EventBatch, SdkError> {
        let mut catchup_events = self.catchup_events.lock().expect("catchup events mutex");
        let events = (0..max).filter_map(|_| catchup_events.pop_front()).collect::<Vec<_>>();
        let next_seq = events.last().map_or(0, |event| event.seq_no);
        Ok(from_json::<EventBatch>(json!({
            "events": events,
            "next_cursor": format!("v2:bench-runtime:default:{next_seq}"),
            "dropped_count": 0,
            "snapshot_high_watermark_seq_no": null,
            "extensions": {}
        })))
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        BenchBackend::default().snapshot()
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        BenchBackend::default().shutdown(mode)
    }
}

impl SdkBackend for SlowSubscriberBenchBackend {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        BenchBackend::default().negotiate(req)
    }

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        BenchBackend::default().send(req)
    }

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        BenchBackend::default().cancel(id)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        BenchBackend::default().status(id)
    }

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError> {
        BenchBackend::default().configure(expected_revision, patch)
    }

    fn poll_events(
        &self,
        _cursor: Option<EventCursor>,
        _max: usize,
    ) -> Result<EventBatch, SdkError> {
        Ok(from_json::<EventBatch>(json!({
            "events": [],
            "next_cursor": "v2:bench-runtime:default:0",
            "dropped_count": 0,
            "snapshot_high_watermark_seq_no": null,
            "extensions": {}
        })))
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        BenchBackend::default().snapshot()
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        BenchBackend::default().shutdown(mode)
    }
}

impl SdkBackendAsyncEvents for BenchBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        Ok(from_json::<EventSubscription>(json!({
            "start": start,
            "cursor": null
        })))
    }

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        Ok(Some(Box::pin(tokio_stream::iter(vec![Ok(bench_sdk_event(seq))]))))
    }
}

impl SdkBackendAsyncEvents for StreamBenchBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        Ok(from_json::<EventSubscription>(json!({
            "start": start,
            "cursor": null
        })))
    }

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let events =
            self.live_events.lock().expect("live events mutex").drain(..).collect::<Vec<_>>();
        Ok(Some(Box::pin(tokio_stream::iter(events.into_iter().map(Ok)))))
    }
}

impl SdkBackendAsyncEvents for SlowSubscriberBenchBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        Ok(from_json::<EventSubscription>(json!({
            "start": start,
            "cursor": null
        })))
    }

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let stats = Arc::clone(&self.stats);
        let event_count = self.event_count;
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<SdkEvent, SdkError>>(1);
        tokio::spawn(async move {
            for seq in 1..=event_count {
                stats.producer_pending.store(true, Ordering::Relaxed);
                stats.observe();
                if tx.send(Ok(bench_sdk_event(seq))).await.is_err() {
                    break;
                }
                stats.producer_pending.store(false, Ordering::Relaxed);
                stats.queued.fetch_add(1, Ordering::Relaxed);
                stats.observe();
            }
        });
        Ok(Some(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))))
    }
}

impl SdkBackendAsyncOps for BenchBackend {
    fn negotiate_async(
        &self,
        req: NegotiationRequest,
    ) -> lxmf_sdk::SdkBoxFuture<'_, NegotiationResponse> {
        Box::pin(async move { self.negotiate(req) })
    }

    fn send_async(&self, req: SendRequest) -> lxmf_sdk::SdkBoxFuture<'_, MessageId> {
        Box::pin(async move { self.send(req) })
    }

    fn status_async(&self, id: MessageId) -> lxmf_sdk::SdkBoxFuture<'_, Option<DeliverySnapshot>> {
        Box::pin(async move { self.status(id) })
    }

    fn snapshot_async(&self) -> lxmf_sdk::SdkBoxFuture<'_, RuntimeSnapshot> {
        Box::pin(async move { self.snapshot() })
    }

    fn shutdown_async(&self, mode: ShutdownMode) -> lxmf_sdk::SdkBoxFuture<'_, Ack> {
        Box::pin(async move { self.shutdown(mode) })
    }
}

impl SdkBackendAsyncOps for StreamBenchBackend {
    fn negotiate_async(
        &self,
        req: NegotiationRequest,
    ) -> lxmf_sdk::SdkBoxFuture<'_, NegotiationResponse> {
        Box::pin(async move { self.negotiate(req) })
    }

    fn send_async(&self, req: SendRequest) -> lxmf_sdk::SdkBoxFuture<'_, MessageId> {
        Box::pin(async move { self.send(req) })
    }

    fn status_async(&self, id: MessageId) -> lxmf_sdk::SdkBoxFuture<'_, Option<DeliverySnapshot>> {
        Box::pin(async move { self.status(id) })
    }

    fn snapshot_async(&self) -> lxmf_sdk::SdkBoxFuture<'_, RuntimeSnapshot> {
        Box::pin(async move { self.snapshot() })
    }

    fn shutdown_async(&self, mode: ShutdownMode) -> lxmf_sdk::SdkBoxFuture<'_, Ack> {
        Box::pin(async move { self.shutdown(mode) })
    }
}

impl SdkBackendAsyncOps for SlowSubscriberBenchBackend {
    fn negotiate_async(
        &self,
        req: NegotiationRequest,
    ) -> lxmf_sdk::SdkBoxFuture<'_, NegotiationResponse> {
        Box::pin(async move { self.negotiate(req) })
    }

    fn send_async(&self, req: SendRequest) -> lxmf_sdk::SdkBoxFuture<'_, MessageId> {
        Box::pin(async move { self.send(req) })
    }

    fn status_async(&self, id: MessageId) -> lxmf_sdk::SdkBoxFuture<'_, Option<DeliverySnapshot>> {
        Box::pin(async move { self.status(id) })
    }

    fn snapshot_async(&self) -> lxmf_sdk::SdkBoxFuture<'_, RuntimeSnapshot> {
        Box::pin(async move { self.snapshot() })
    }

    fn shutdown_async(&self, mode: ShutdownMode) -> lxmf_sdk::SdkBoxFuture<'_, Ack> {
        Box::pin(async move { self.shutdown(mode) })
    }
}

fn bench_sdk_event(seq: u64) -> SdkEvent {
    from_json::<SdkEvent>(json!({
        "event_id": format!("bench-event-{seq}"),
        "runtime_id": "bench-runtime",
        "stream_id": "default",
        "seq_no": seq,
        "contract_version": 2,
        "ts_ms": seq,
        "event_type": "RuntimeStateChanged",
        "severity": Severity::Info,
        "source_component": "bench",
        "operation_id": null,
        "message_id": null,
        "peer_id": null,
        "correlation_id": null,
        "trace_id": null,
        "payload": { "from": "running", "to": "running" },
        "extensions": {}
    }))
}

fn sample_start_request() -> StartRequest {
    let config = from_json::<SdkConfig>(json!({
        "profile": Profile::DesktopFull,
        "bind_mode": BindMode::LocalOnly,
        "auth_mode": AuthMode::LocalTrusted,
        "overflow_policy": OverflowPolicy::Reject,
        "block_timeout_ms": null,
        "event_stream": from_json::<EventStreamConfig>(json!({
            "max_poll_events": 256,
            "max_event_bytes": 65_536,
            "max_batch_bytes": 1_048_576,
            "max_extension_keys": 32
        })),
        "idempotency_ttl_ms": 86_400_000,
        "redaction": from_json::<RedactionConfig>(json!({
            "enabled": true,
            "sensitive_transform": RedactionTransform::Hash,
            "break_glass_allowed": false,
            "break_glass_ttl_ms": null
        })),
        "rpc_backend": null,
        "extensions": {}
    }));
    from_json::<StartRequest>(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": ["sdk.capability.cursor_replay"],
        "config": config
    }))
}

fn sample_send_request(counter: u64) -> SendRequest {
    from_json::<SendRequest>(json!({
        "source": "bench-src",
        "destination": "bench-dst",
        "payload": {
            "content": "benchmark send",
            "sequence": counter
        },
        "idempotency_key": null,
        "ttl_ms": null,
        "correlation_id": null,
        "extensions": {}
    }))
}

fn from_json<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("benchmark fixture json must deserialize")
}

fn bench_start(c: &mut Criterion) {
    c.bench_function("lxmf_sdk/start", |b| {
        b.iter_batched(
            || (Client::new(BenchBackend::default()), sample_start_request()),
            |(client, request)| {
                let handle = client.start(request).expect("start must succeed");
                black_box(handle);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_send(c: &mut Criterion) {
    let client = Client::new(BenchBackend::default());
    client.start(sample_start_request()).expect("start must succeed");
    let counter = AtomicU64::new(0);

    c.bench_function("lxmf_sdk/send", |b| {
        b.iter(|| {
            let seq = counter.fetch_add(1, Ordering::Relaxed);
            let message_id = client.send(sample_send_request(seq)).expect("send must succeed");
            black_box(message_id);
        });
    });
}

fn bench_async_send(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let client = Client::new(BenchBackend::default());
    runtime.block_on(async {
        client.start_async(sample_start_request()).await.expect("async start must succeed");
    });
    let counter = AtomicU64::new(0);

    c.bench_function("lxmf_sdk/async_send", |b| {
        b.iter(|| {
            let seq = counter.fetch_add(1, Ordering::Relaxed);
            let message_id = runtime
                .block_on(async { client.send_async(sample_send_request(seq)).await })
                .expect("async send must succeed");
            black_box(message_id);
        });
    });
}

fn bench_poll_and_snapshot(c: &mut Criterion) {
    let client = Client::new(BenchBackend::default());
    client.start(sample_start_request()).expect("start must succeed");

    c.bench_function("lxmf_sdk/poll_events", |b| {
        b.iter(|| {
            let batch = client.poll_events(None, 64).expect("poll must succeed");
            black_box(batch);
        });
    });

    c.bench_function("lxmf_sdk/snapshot", |b| {
        b.iter(|| {
            let snapshot = client.snapshot().expect("snapshot must succeed");
            black_box(snapshot);
        });
    });
}

fn bench_app_event_stream(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let client = AppClient::new(BenchBackend::default());
    client.runtime().start(AppConfig::desktop_default()).expect("app start must succeed");

    c.bench_function("lxmf_sdk/app_event_stream_next", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let mut events = client
                    .events()
                    .subscribe(AppSubscriptionStart::Tail)
                    .expect("subscribe must succeed");
                let event =
                    events.next().await.expect("stream should yield").expect("event should parse");
                black_box(event);
            });
        });
    });
}

fn bench_event_fanout_latency(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    c.bench_function("lxmf_sdk/event_fanout_128", |b| {
        b.iter_batched(
            || {
                let client = AppClient::new(StreamBenchBackend::fanout(128));
                client.runtime().start(AppConfig::desktop_default()).expect("app start");
                client.events().subscribe(AppSubscriptionStart::Head).expect("subscribe")
            },
            |mut events| {
                runtime.block_on(async {
                    let mut observed = 0_u64;
                    while observed < 128 {
                        let event = events
                            .next()
                            .await
                            .expect("stream should yield")
                            .expect("event should parse");
                        observed = event.metadata.seq_no;
                    }
                    black_box(observed);
                });
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_reconnect_catchup(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    c.bench_function("lxmf_sdk/reconnect_catchup_64", |b| {
        b.iter_batched(
            || {
                let client = AppClient::new(StreamBenchBackend::reconnect_catchup(1, 64));
                client.runtime().start(AppConfig::desktop_default()).expect("app start");
                client.events().subscribe(AppSubscriptionStart::Head).expect("subscribe")
            },
            |mut events| {
                runtime.block_on(async {
                    let mut observed = 0_u64;
                    while observed < 65 {
                        let event = events
                            .next()
                            .await
                            .expect("stream should yield")
                            .expect("event should parse");
                        observed = event.metadata.seq_no;
                    }
                    black_box(observed);
                });
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_slow_subscriber_memory(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

    c.bench_function("lxmf_sdk/slow_subscriber_memory_bound", |b| {
        b.iter_batched(
            || {
                let stats = Arc::new(SlowSubscriberStats::default());
                let client =
                    AppClient::new(SlowSubscriberBenchBackend::new(64, Arc::clone(&stats)));
                client.runtime().start(AppConfig::desktop_default()).expect("app start");
                (client, stats)
            },
            |(client, stats)| {
                runtime.block_on(async {
                    let mut events =
                        client.events().subscribe(AppSubscriptionStart::Head).expect("subscribe");
                    let mut observed = 0_u64;
                    while observed < 64 {
                        let event = events
                            .next()
                            .await
                            .expect("stream should yield")
                            .expect("event should parse");
                        let _ = stats.queued.fetch_update(
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                            |queued| Some(queued.saturating_sub(1)),
                        );
                        stats.observe();
                        observed = event.metadata.seq_no;
                        tokio::time::sleep(Duration::from_micros(50)).await;
                    }
                    black_box((observed, stats.peak_buffered.load(Ordering::Relaxed)));
                });
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_start,
    bench_send,
    bench_async_send,
    bench_poll_and_snapshot,
    bench_app_event_stream,
    bench_event_fanout_latency,
    bench_reconnect_catchup,
    bench_slow_subscriber_memory
);
criterion_main!(benches);
