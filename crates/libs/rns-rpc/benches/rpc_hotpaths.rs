use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use rns_rpc::{
    EventSinkBridge, MessageRecord, MessagesStore, OutboundBridge, OutboundDeliveryOptions,
    RpcDaemon, RpcEvent, RpcEventSinkEnvelope, RpcRequest,
};
use serde_json::{json, Value as JsonValue};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

struct NoopEventSink;
struct NoopOutboundBridge;

impl OutboundBridge for NoopOutboundBridge {
    fn deliver(
        &self,
        record: &MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        black_box((record, options));
        Ok(())
    }
}

impl EventSinkBridge for NoopEventSink {
    fn sink_id(&self) -> &str {
        "bench-noop"
    }

    fn sink_kind(&self) -> &'static str {
        "bench"
    }

    fn publish(&self, envelope: &RpcEventSinkEnvelope) -> Result<(), std::io::Error> {
        black_box(envelope);
        Ok(())
    }
}

fn rpc_request(id: u64, method: &str, params: JsonValue) -> RpcRequest {
    RpcRequest { id, method: method.to_string(), params: Some(params) }
}

fn bench_send_message_v2(c: &mut Criterion) {
    let sequence = AtomicU64::new(0);
    c.bench_function("rns_rpc/send_message_v2", |b| {
        b.iter_batched(
            || {
                let daemon = RpcDaemon::test_instance();
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                let req = rpc_request(
                    seq + 1,
                    "send_message_v2",
                    json!({
                        "id": format!("bench-send-{seq}"),
                        "source": "bench-src",
                        "destination": "bench-dst",
                        "title": "",
                        "content": "benchmark payload",
                        "fields": null,
                        "method": "direct"
                    }),
                );
                (daemon, req)
            },
            |(daemon, req)| {
                let response = daemon.handle_rpc(req).expect("send_message_v2 should succeed");
                black_box(response);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_send_message_v2_bridge_schedule(c: &mut Criterion) {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "bench-bridge-node".to_string(),
        Some(Arc::new(NoopOutboundBridge)),
        None,
    );
    let sequence = AtomicU64::new(0);
    c.bench_function("rns_rpc/send_message_v2_bridge_schedule", |b| {
        b.iter(|| {
            let seq = sequence.fetch_add(1, Ordering::Relaxed);
            let request = rpc_request(
                seq + 100_000,
                "send_message_v2",
                json!({
                    "id": format!("bench-bridge-send-{seq}"),
                    "source": "bench-src",
                    "destination": "bench-dst",
                    "title": "",
                    "content": "benchmark payload",
                    "fields": null,
                    "method": "direct"
                }),
            );
            let response =
                daemon.handle_rpc(request).expect("bridge-backed send_message_v2 should succeed");
            black_box(response);
        });
    });
    black_box(daemon);
}

fn bench_poll_events_v2(c: &mut Criterion) {
    let daemon = RpcDaemon::test_instance();
    daemon.emit_event(RpcEvent {
        event_type: "bench_event".to_string(),
        payload: json!({ "value": 1 }),
    });
    let request = rpc_request(10, "sdk_poll_events_v2", json!({ "cursor": null, "max": 1 }));

    c.bench_function("rns_rpc/sdk_poll_events_v2", |b| {
        b.iter(|| {
            let response = daemon
                .handle_rpc(black_box(request.clone()))
                .expect("sdk_poll_events_v2 should succeed");
            black_box(response);
        });
    });
}

fn bench_snapshot_v2(c: &mut Criterion) {
    let daemon = RpcDaemon::test_instance();
    let request = rpc_request(20, "sdk_snapshot_v2", json!({ "include_counts": true }));
    c.bench_function("rns_rpc/sdk_snapshot_v2", |b| {
        b.iter(|| {
            let response = daemon
                .handle_rpc(black_box(request.clone()))
                .expect("sdk_snapshot_v2 should succeed");
            black_box(response);
        });
    });
}

fn bench_topic_create_v2(c: &mut Criterion) {
    let sequence = AtomicU64::new(0);
    c.bench_function("rns_rpc/sdk_topic_create_v2", |b| {
        b.iter_batched(
            || {
                let daemon = RpcDaemon::test_instance();
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                let request = rpc_request(
                    seq + 30,
                    "sdk_topic_create_v2",
                    json!({
                        "topic_path": format!("bench/topic/{seq}"),
                        "metadata": { "bench": true },
                        "extensions": {}
                    }),
                );
                (daemon, request)
            },
            |(daemon, request)| {
                let response =
                    daemon.handle_rpc(request).expect("sdk_topic_create_v2 should succeed");
                black_box(response);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_accept_inbound(c: &mut Criterion) {
    let sequence = AtomicU64::new(0);
    c.bench_function("rns_rpc/accept_inbound", |b| {
        b.iter_batched(
            || {
                let daemon = RpcDaemon::test_instance();
                let seq = sequence.fetch_add(1, Ordering::Relaxed);
                let record = MessageRecord {
                    id: format!("bench-inbound-{seq}"),
                    source: "bench-src".into(),
                    destination: "bench-dst".into(),
                    title: "bench-title".into(),
                    content: "benchmark inbound payload".into(),
                    timestamp: seq as i64,
                    direction: "in".into(),
                    fields: None,
                    receipt_status: None,
                };
                (daemon, record)
            },
            |(daemon, record)| {
                daemon.accept_inbound(record).expect("accept_inbound should succeed");
                black_box(daemon);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_event_sink_dispatch(c: &mut Criterion) {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let daemon = RpcDaemon::with_store_and_bridges_and_sinks(
        store,
        "bench-sink-node".to_string(),
        None,
        None,
        vec![Arc::new(NoopEventSink)],
    );
    daemon
        .handle_rpc(rpc_request(
            1,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "allow_kinds": ["bench"]
                    }
                }
            }),
        ))
        .expect("configure event sink");
    let sequence = AtomicU64::new(0);
    c.bench_function("rns_rpc/event_sink_dispatch", |b| {
        b.iter(|| {
            let seq = sequence.fetch_add(1, Ordering::Relaxed);
            daemon.emit_event(RpcEvent {
                event_type: "bench_event".to_string(),
                payload: json!({ "seq": seq }),
            });
        });
    });
    black_box(daemon);
}

criterion_group!(
    benches,
    bench_send_message_v2,
    bench_send_message_v2_bridge_schedule,
    bench_poll_events_v2,
    bench_snapshot_v2,
    bench_topic_create_v2,
    bench_accept_inbound,
    bench_event_sink_dispatch
);
criterion_main!(benches);
