use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use rns_rpc::{RpcDaemon, RpcEvent, RpcRequest};
use serde_json::{json, Value as JsonValue};
use std::sync::atomic::{AtomicU64, Ordering};

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

criterion_group!(
    benches,
    bench_send_message_v2,
    bench_poll_events_v2,
    bench_snapshot_v2,
    bench_topic_create_v2
);
criterion_main!(benches);
