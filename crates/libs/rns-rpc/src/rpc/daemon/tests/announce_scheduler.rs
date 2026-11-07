#[derive(Default)]
struct RecordingAnnounceBridge {
    full_count: std::sync::Mutex<u32>,
    delivery_hashes: std::sync::Mutex<Vec<String>>,
    display_names: std::sync::Mutex<Vec<String>>,
}

impl RecordingAnnounceBridge {
    fn full_count(&self) -> u32 {
        *self.full_count.lock().expect("full count mutex poisoned")
    }

    fn delivery_hashes(&self) -> Vec<String> {
        self.delivery_hashes.lock().expect("delivery hashes mutex poisoned").clone()
    }
}

impl AnnounceBridge for RecordingAnnounceBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        *self.full_count.lock().expect("full count mutex poisoned") += 1;
        Ok(())
    }

    fn announce_delivery(&self, destination_hash: &str) -> Result<(), std::io::Error> {
        self.delivery_hashes
            .lock()
            .expect("delivery hashes mutex poisoned")
            .push(destination_hash.to_string());
        Ok(())
    }

    fn set_display_name(&self, display_name: &str) -> Result<(), std::io::Error> {
        self.display_names
            .lock()
            .expect("display names mutex poisoned")
            .push(display_name.to_string());
        Ok(())
    }
}

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

#[test]
fn announce_delivery_calls_targeted_bridge_and_publishes_delivery_event() {
    let bridge = std::sync::Arc::new(RecordingAnnounceBridge::default());
    let daemon = RpcDaemon::with_store_and_bridges(
        MessagesStore::in_memory().expect("store"),
        hex::encode([0x21; 16]),
        None,
        Some(bridge.clone()),
    );
    let destination_hash = hex::encode([0x42; 16]);
    daemon.set_delivery_destination_hash(Some(destination_hash.clone()));

    let response = daemon
        .handle_rpc(rpc_request(
            201,
            "announce_delivery",
            json!({ "destination_hash": destination_hash.to_ascii_uppercase() }),
        ))
        .expect("announce delivery response");

    assert!(response.error.is_none());
    let result = response.result.expect("announce delivery result");
    assert_eq!(result["announce_id"], json!(201));
    assert_eq!(result["scope"], json!("delivery"));
    assert_eq!(result["destination_hash"], json!(destination_hash));
    assert_eq!(bridge.full_count(), 0);
    assert_eq!(bridge.delivery_hashes(), vec![destination_hash.clone()]);

    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.event_type, "announce_sent");
    assert_eq!(event.payload["announce_id"], json!(201));
    assert_eq!(event.payload["scope"], json!("delivery"));
    assert!(event.payload["destination_hash"]
        .as_str()
        .is_some_and(|value| value.starts_with("sha256:") && value != destination_hash));
}

#[test]
fn announce_delivery_rejects_nonlocal_delivery_destination() {
    let bridge = std::sync::Arc::new(RecordingAnnounceBridge::default());
    let daemon = RpcDaemon::with_store_and_bridges(
        MessagesStore::in_memory().expect("store"),
        hex::encode([0x21; 16]),
        None,
        Some(bridge.clone()),
    );
    daemon.set_delivery_destination_hash(Some(hex::encode([0x42; 16])));

    let response = daemon
        .handle_rpc(rpc_request(
            202,
            "announce_delivery",
            json!({ "destination_hash": hex::encode([0x43; 16]) }),
        ))
        .expect("announce delivery response");

    let error = response.error.expect("nonlocal destination error");
    assert_eq!(error.code, "DELIVERY_DESTINATION_NOT_FOUND");
    assert!(response.result.is_none());
    assert_eq!(bridge.full_count(), 0);
    assert!(bridge.delivery_hashes().is_empty());
    assert!(daemon.take_event().is_none());
}

#[test]
fn set_display_name_updates_bridge_and_requires_a_name() {
    let bridge = std::sync::Arc::new(RecordingAnnounceBridge::default());
    let daemon = RpcDaemon::with_store_and_bridges(
        MessagesStore::in_memory().expect("store"),
        hex::encode([0x21; 16]),
        None,
        Some(bridge.clone()),
    );

    let response = daemon
        .handle_rpc(rpc_request(202, "set_display_name", json!({ "display_name": "  Corvo  " })))
        .expect("set display name response");
    assert!(response.error.is_none());
    assert_eq!(response.result.expect("result")["applied"], json!(true));
    assert_eq!(
        bridge.display_names.lock().expect("display names mutex poisoned").clone(),
        vec!["Corvo".to_string()]
    );

    let missing = daemon.handle_rpc(rpc_request(203, "set_display_name", json!({ "display_name": " " })));
    assert!(missing.is_err());
    assert_eq!(bridge.full_count(), 0);
}
