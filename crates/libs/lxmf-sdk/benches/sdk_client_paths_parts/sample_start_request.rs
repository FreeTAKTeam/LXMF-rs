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
                        let mut queued = stats.queued.load(Ordering::Relaxed);
                        loop {
                            match stats.queued.compare_exchange_weak(
                                queued,
                                queued.saturating_sub(1),
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            ) {
                                Ok(_) => break,
                                Err(current) => queued = current,
                            }
                        }
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
