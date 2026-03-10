#[test]
fn shared_announce_scheduler_publishes_queued_events() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let daemon = std::sync::Arc::new(RpcDaemon::test_instance());
        let handle = daemon.clone().start_announce_scheduler_shared(1);

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let response = daemon
                    .handle_rpc(rpc_request(
                        200,
                        "sdk_poll_events_v2",
                        json!({
                            "cursor": null,
                            "max": 8
                        }),
                    ))
                    .expect("poll");
                let result = response.result.expect("result");
                let events = result["events"].as_array().expect("events");
                if events.iter().any(|event| {
                    event.get("event_type").and_then(JsonValue::as_str) == Some("announce_sent")
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("announce_sent should appear in sdk event log");

        handle.abort();
        let _ = handle.await;
    });
}
