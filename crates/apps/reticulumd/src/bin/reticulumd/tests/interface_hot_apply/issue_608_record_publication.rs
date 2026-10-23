use super::*;

#[tokio::test]
async fn native_worker_normalizes_runtime_metadata_and_finishes_reload_after_snapshot_detach() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn_with_daemon(
        manager.clone(),
        Vec::new(),
        Arc::downgrade(&daemon),
    );
    daemon.set_interface_mutation_bridge(Arc::new(bridge));
    let mut record = udp_record("metadata", "127.0.0.1", 0);
    record.settings = Some(json!({"_runtime": {"udp": "malformed", "tcp": [], "pipe": false}}));
    let apply = |id, method: &str, records: Vec<InterfaceRecord>| {
        let response = daemon
            .handle_rpc(RpcRequest {
                id,
                method: method.to_string(),
                params: Some(json!({"interfaces": records})),
            })
            .expect("real interface command");
        assert!(response.error.is_none(), "{response:?}");
        let result = response.result.expect("queue acknowledgement");
        assert_eq!(result["queued"], true);
        assert_eq!(result["complete"], false);
        result
    };
    apply(1, "set_interfaces", vec![record.clone()]);
    let active = wait_for_metadata_state(&daemon, "spawned").await;
    let first_iface = active.settings.as_ref().expect("settings")["_runtime"]["iface"].clone();
    assert!(first_iface.is_string());
    assert!(active.settings.as_ref().expect("settings")["_runtime"]["udp"]["status"].is_object());
    let mut disabled = active;
    disabled.enabled = false;
    apply(2, "set_interfaces", vec![disabled]);
    let detached = wait_for_metadata_state(&daemon, "detached").await;
    let runtime = &detached.settings.as_ref().expect("settings")["_runtime"];
    assert_eq!(runtime["runtime_status"], "stopped");
    assert!(runtime["iface"].is_null());
    assert!(runtime["udp"].is_null(), "no stale carrier binding after snapshot detach");
    assert!(manager.lock().await.interface_hashes().is_empty());

    let response = apply(3, "reload_config", vec![record]);
    let reloaded = wait_for_metadata_state(&daemon, "spawned").await;
    assert_ne!(reloaded.settings.as_ref().expect("settings")["_runtime"]["iface"], first_iface);
    let events = daemon
        .handle_rpc(RpcRequest {
            id: 4,
            method: "sdk_poll_events_v2".to_string(),
            params: Some(json!({"cursor": null, "max": 16})),
        })
        .expect("replay")
        .result
        .expect("events");
    let events = events["events"].as_array().expect("events");
    let reloads =
        events.iter().filter(|event| event["event_type"] == "config_reloaded").collect::<Vec<_>>();
    assert_eq!(reloads.len(), 1, "reload completion is deferred, not lost");
    assert_eq!(reloads[0]["payload"]["timestamp"], response["timestamp"]);
    let completed = events
        .iter()
        .filter(|event| event["event_type"] == "interfaces_updated")
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 3);
    assert_eq!(
        completed[2]["payload"]["interfaces"][0]["settings"]["_runtime"]["iface"],
        reloaded.settings.as_ref().expect("settings")["_runtime"]["iface"]
    );
    apply(5, "set_interfaces", vec![]);
    timeout(Duration::from_secs(2), async {
        while !daemon.interface_records().is_empty()
            || !manager.lock().await.interface_hashes().is_empty()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cleanup after later native command");

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("TCP status peer");
    apply(
        6,
        "set_interfaces",
        vec![tcp_record("stream", "127.0.0.1", listener.local_addr().expect("TCP address").port())],
    );
    let (_stream, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("native TCP connection")
        .expect("accept");
    let stream = wait_for_metadata_state(&daemon, "spawned").await;
    let runtime = &stream.settings.as_ref().expect("stream settings")["_runtime"];
    assert!(runtime["tcp"]["stream_status"].is_object(), "established daemon status schema");
    assert!(runtime["tcp"]["status"].is_null());
    apply(7, "set_interfaces", vec![]);
    timeout(Duration::from_secs(2), async {
        while !daemon.interface_records().is_empty()
            || !manager.lock().await.interface_hashes().is_empty()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("TCP worker cleanup");
}

async fn wait_for_metadata_state(daemon: &RpcDaemon, state: &str) -> InterfaceRecord {
    timeout(Duration::from_secs(2), async {
        loop {
            if let Some(record) = daemon.interface_records().into_iter().find(|record| {
                record
                    .settings
                    .as_ref()
                    .is_some_and(|settings| settings["_runtime"]["startup_status"] == state)
            }) {
                break record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native worker publishes requested lifecycle state")
}

#[tokio::test]
async fn queued_policy_rejection_publishes_old_active_policy_and_preserves_other_records() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let parent = manager.lock().await.new_channel(8);
    let address = *parent.address();
    let child = manager.lock().await.new_channel(8);
    let old_config = InterfaceSharedConfig {
        network_name: Some("old-network".to_string()),
        ..Default::default()
    };
    {
        let mut manager = manager.lock().await;
        assert!(manager.set_shared_config(address, old_config.clone()));
        assert!(manager.set_gravity(address, 7));
        assert!(manager.inherit_runtime_config(address, *child.address()));
    }
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = child.ifac_state.write().expect("healthy child IFAC state");
        panic!("poison later child before native policy publication");
    }));
    assert!(poisoned.is_err());

    let mut active = tcp_record("active", "127.0.0.1", 4242);
    active.settings = Some(json!({"network_name": "old-network", "gravity": 7}));
    let mut disabled = udp_record("disabled", "127.0.0.1", 4243);
    disabled.enabled = false;
    daemon.replace_interfaces(vec![active.clone(), disabled.clone()]);
    let bridge = InterfaceHotApplyBridge::spawn_with_daemon(
        manager.clone(),
        vec![("active".to_string(), active.clone(), address)],
        Arc::downgrade(&daemon),
    );
    daemon.set_interface_mutation_bridge(Arc::new(bridge.clone()));
    let mut requested = active.clone();
    requested.settings = Some(json!({
        "network_name": "new-network", "gravity": 99, "outgoing": false,
        "mode": "boundary", "bitrate": 1200, "announce_cap": 1
    }));
    let response = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "set_interfaces".to_string(),
            params: Some(json!({"interfaces": [requested, disabled]})),
        })
        .expect("queue real native mutation");
    assert!(response.error.is_none());
    let result = response.result.expect("queue acknowledgement");
    assert_eq!(result["queued"], true);
    assert_eq!(result["complete"], false);
    assert_eq!(result["updated"], false);
    assert_eq!(result["applied_interfaces"], json!([]));

    let committed = timeout(Duration::from_secs(2), async {
        loop {
            let records = daemon.interface_records();
            if records[0].settings.as_ref().expect("active settings")["_runtime"]["apply"]["state"]
                == "rejected"
            {
                break records;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("worker publishes its real rejection");
    assert_eq!(committed.len(), 2);
    assert_eq!(committed[0].name.as_deref(), Some("active"));
    let settings = committed[0].settings.as_ref().expect("committed active settings");
    assert_eq!(settings["network_name"], "old-network");
    assert_eq!(settings["gravity"], 7);
    assert!(!committed[1].enabled);
    assert_eq!(committed[1].name.as_deref(), Some("disabled"));
    {
        let manager = manager.lock().await;
        assert_eq!(manager.shared_config(&address), Some(&old_config));
        assert_eq!(manager.shared_config(child.address()), Some(&old_config));
        assert_eq!(manager.gravity(&address), Some(7));
        assert_eq!(manager.mode(&address), Some(InterfaceMode::Full));
        assert_eq!(manager.outgoing(&address), Some(true));
        assert_eq!(manager.announce_pacing(&address), Some((62_500, 2)));
    }
    let replay = daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "sdk_poll_events_v2".to_string(),
            params: Some(json!({"cursor": null, "max": 16})),
        })
        .expect("real event replay")
        .result
        .expect("event batch");
    let events = replay["events"].as_array().expect("events");
    let updates = events
        .iter()
        .filter(|event| event["event_type"] == "interfaces_updated")
        .collect::<Vec<_>>();
    assert_eq!(updates.len(), 1, "only the native owner emits completion");
    assert_eq!(updates[0]["payload"]["queued"], false);
    assert_eq!(updates[0]["payload"]["interfaces"][0]["settings"]["network_name"], "old-network");

    let mut unmanaged = udp_record("startup-only", "127.0.0.1", 4244);
    unmanaged.kind = "serial".to_string();
    let mut next = committed;
    next.insert(1, unmanaged.clone());
    bridge.apply_interfaces(next).expect("queue complete owner snapshot");
    timeout(Duration::from_secs(2), async {
        while daemon.interface_records().len() != 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("unmanaged record retained");
    assert_eq!(daemon.interface_records()[1], unmanaged, "submitted order and record preserved");

    bridge.apply_interfaces(Vec::new()).expect("queue worker cleanup");
    timeout(Duration::from_secs(2), async {
        while !manager.lock().await.interface_hashes().is_empty()
            || daemon.interface_records() != vec![unmanaged.clone()]
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("owned tree removed after failed configuration");
    assert_eq!(daemon.interface_records(), vec![unmanaged], "startup-only record is retained");
}

#[tokio::test]
async fn queued_native_replacements_publish_in_order_and_a_failed_replacement_is_inactive() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn_with_daemon(
        manager.clone(),
        Vec::new(),
        Arc::downgrade(&daemon),
    );
    daemon.set_interface_mutation_bridge(Arc::new(bridge));
    let mut unmanaged = udp_record("startup-only", "127.0.0.1", 4244);
    unmanaged.kind = "serial".to_string();
    daemon.replace_interfaces(vec![unmanaged.clone()]);
    let first = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("first port");
    let second = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("second port");
    let first_port = first.local_addr().expect("first address").port();
    let second_port = second.local_addr().expect("second address").port();
    drop((first, second));
    // Hold the real manager to prevent any native mutation from committing
    // while both synchronous RPC commands acknowledge queue admission.
    let held = manager.lock().await;
    for (index, record) in [
        udp_record("queued", "127.0.0.1", first_port),
        udp_record("queued", "127.0.0.1", second_port),
    ]
    .into_iter()
    .enumerate()
    {
        let response = daemon
            .handle_rpc(RpcRequest {
                id: index as u64,
                method: "set_interfaces".to_string(),
                params: Some(json!({"interfaces": [record]})),
            })
            .expect("queue native replacement");
        assert!(response.error.is_none());
        assert_eq!(response.result.expect("acknowledgement")["complete"], false);
        assert_eq!(daemon.interface_records(), vec![unmanaged.clone()]);
    }
    drop(held);
    native_completion_events(&daemon, 2).await;
    let mut copied = daemon
        .interface_records()
        .into_iter()
        .find(|record| record.name.as_deref() == Some("queued"))
        .expect("live snapshot");
    assert!(copied.settings.as_ref().expect("runtime settings")["_runtime"]["udp"]["status"]
        .is_object());
    // This manager-only daemon has no multicast Transport peer-routing owner.
    copied.host = Some("239.255.0.1".to_string());
    let response = daemon
        .handle_rpc(RpcRequest {
            id: 3,
            method: "set_interfaces".to_string(),
            params: Some(json!({"interfaces": [copied]})),
        })
        .expect("replace copied live snapshot");
    assert!(response.error.is_none());
    assert_eq!(response.result.expect("queue acknowledgement")["complete"], false);
    let events = native_completion_events(&daemon, 3).await;
    let committed = |event: &serde_json::Value| {
        event["payload"]["interfaces"]
            .as_array()
            .expect("snapshot")
            .iter()
            .find(|record| record["name"] == "queued")
            .expect("command record")
            .clone()
    };
    let first = committed(&events[0]);
    let second = committed(&events[1]);
    let failed = committed(&events[2]);
    assert_eq!(first["port"], first_port);
    assert_eq!(second["port"], second_port);
    assert_ne!(first["settings"]["_runtime"]["iface"], second["settings"]["_runtime"]["iface"]);
    assert_eq!(failed["host"], "239.255.0.1");
    assert_eq!(failed["settings"]["_runtime"]["startup_status"], "failed");
    assert!(failed["settings"]["_runtime"]["iface"].is_null());
    assert!(failed["settings"]["_runtime"]["udp"].is_null(), "no stale copied carrier state");
    assert!(manager.lock().await.interface_hashes().is_empty(), "old worker was removed");
    let final_records = daemon.interface_records();
    assert_eq!(final_records[0], unmanaged);
    assert_eq!(final_records.len(), 2);
}

async fn native_completion_events(daemon: &RpcDaemon, count: usize) -> Vec<serde_json::Value> {
    timeout(Duration::from_secs(3), async {
        loop {
            let batch = daemon
                .handle_rpc(RpcRequest {
                    id: 9,
                    method: "sdk_poll_events_v2".to_string(),
                    params: Some(json!({"cursor": null, "max": 16})),
                })
                .expect("replay native completion")
                .result
                .expect("event batch");
            let events = batch["events"]
                .as_array()
                .expect("events")
                .iter()
                .filter(|event| event["event_type"] == "interfaces_updated")
                .cloned()
                .collect::<Vec<_>>();
            if events.len() == count {
                break events;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("native commands complete in order")
}
