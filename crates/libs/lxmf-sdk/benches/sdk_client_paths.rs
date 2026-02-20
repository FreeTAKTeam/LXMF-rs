use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use lxmf_sdk::backend::SdkBackend;
use lxmf_sdk::capability::{EffectiveLimits, NegotiationRequest, NegotiationResponse};
use lxmf_sdk::event::{EventBatch, EventCursor};
use lxmf_sdk::profiles::required_capabilities;
use lxmf_sdk::types::{
    Ack, AuthMode, BindMode, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState,
    EventStreamConfig, MessageId, OverflowPolicy, Profile, RedactionConfig, RedactionTransform,
    RuntimeSnapshot, RuntimeState, SdkConfig, SendRequest, ShutdownMode, StartRequest,
};
use lxmf_sdk::{Client, LxmfSdk, SdkError};
use serde::de::DeserializeOwned;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct BenchBackend {
    next_id: AtomicU64,
}

impl SdkBackend for BenchBackend {
    fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let mut effective_capabilities = required_capabilities(Profile::DesktopFull)
            .iter()
            .map(|capability| (*capability).to_string())
            .collect::<Vec<_>>();
        effective_capabilities.push("sdk.capability.cursor_replay".to_string());
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
        Ok(EventBatch::empty(EventCursor("bench-cursor".to_string())))
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

criterion_group!(benches, bench_start, bench_send, bench_poll_and_snapshot);
criterion_main!(benches);
