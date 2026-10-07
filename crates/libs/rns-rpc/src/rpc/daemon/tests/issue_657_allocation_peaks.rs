#[test]
fn issue_657_unpeer_notification_does_not_block_event_polling_on_large_history() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x78);
    for index in 1..=1024 {
        let entry = issue_657_entry(index, 16);
        daemon.store.upsert_propagation_entry(&entry).expect("seed");
        daemon.store.mark_peer_received_propagation(&peer, &entry.transient_id).expect("received");
    }
    daemon.event_queue.lock().expect("legacy queue").clear();
    daemon.sdk_event_log.lock().expect("SDK log").clear();
    let mut legacy_events = daemon.subscribe_events();
    let mut sdk_events = daemon.subscribe_sdk_events();
    let response =
        daemon.handle_rpc(rpc_request(1, "peer_unpeer", json!({"peer": peer}))).expect("unpeer");
    let result = response.result.expect("result");
    assert_eq!(result["messages"]["handled_ids"].as_array().expect("reply inventory").len(), 1024);
    let legacy = legacy_events.try_recv().expect("legacy notification");
    let sdk = sdk_events.try_recv().expect("SDK notification");
    assert_eq!(sdk.event, legacy);
    assert_eq!(legacy.event_type, "peer_unpeer");
    assert_eq!(legacy.payload["propagation_cleared"], 1024);
    assert_eq!(legacy.payload["messages"]["incoming"], 1024);
    assert!(legacy.payload["messages"].get("handled_ids").is_none());
    assert!(legacy.payload["messages"].get("unhandled_ids").is_none());
    assert!(serde_json::to_vec(&legacy).expect("serialize notification").len() < 8192);
    let poll = daemon
        .handle_rpc(rpc_request(2, "sdk_poll_events_v2", json!({"cursor": null, "max": 1})))
        .expect("poll");
    assert!(poll.error.is_none(), "large unpeer history must not poison the poll cursor");
    let polled = poll.result.expect("poll result");
    assert_eq!(polled["events"].as_array().expect("events").len(), 1);
    assert!(polled.get("next_cursor").is_some());
}

#[test]
fn issue_657_presence_pages_preserve_order_filters_and_contact_metadata() {
    let daemon = RpcDaemon::test_instance();
    for (peer, seen) in [("c", 20), ("b", 30), ("a", 30), ("stale", 1)] {
        daemon
            .upsert_peer(
                peer.into(),
                seen,
                Vec::new(),
                Some(peer.into()),
                Some("announce".into()),
                None,
            )
            .expect("peer");
        let mut peers = daemon.peers.lock().expect("peers");
        let record = peers.get_mut(peer).expect("peer record");
        record.first_seen = 1;
        record.seen_count = 7;
        record.restored_handled_ids = (1..=1024).map(|id| format!("{id:064x}")).collect();
    }
    daemon.sdk_contacts.lock().expect("contacts").insert(
        "a".into(),
        SdkContactRecord {
            identity: "a".into(),
            display_name: None,
            trust_level: "trusted".into(),
            bootstrap: true,
            updated_ts_ms: 1,
            metadata: JsonMap::new(),
            extensions: JsonMap::new(),
        },
    );
    let first = daemon
        .handle_rpc(rpc_request(
            1,
            "sdk_identity_presence_list_v2",
            json!({
                "limit": 1, "min_last_seen_ts_ms": 10,
            }),
        ))
        .expect("first page")
        .result
        .expect("first result");
    let row = &first["presence_list"]["peers"][0];
    assert_eq!(
        row,
        &json!({"peer_id":"a", "last_seen_ts_ms":30, "first_seen_ts_ms":1,
        "seen_count":7, "name":"a", "name_source":"announce", "trust_level":"trusted",
        "bootstrap":true, "extensions":{}})
    );
    let cursor = &first["presence_list"]["next_cursor"];
    let second = daemon
        .handle_rpc(rpc_request(
            2,
            "sdk_identity_presence_list_v2",
            json!({
                "limit": 2, "min_last_seen_ts_ms": 10, "cursor": cursor,
            }),
        ))
        .expect("second page")
        .result
        .expect("second result");
    let rows = second["presence_list"]["peers"].as_array().expect("rows");
    assert_eq!(
        rows.iter().map(|row| row["peer_id"].as_str().expect("peer")).collect::<Vec<_>>(),
        vec!["b", "c"]
    );
    assert!(second["presence_list"]["next_cursor"].is_null());
    let invalid = daemon
        .handle_rpc(rpc_request(
            3,
            "sdk_identity_presence_list_v2",
            json!({
                "cursor":"presence:4", "min_last_seen_ts_ms":10,
            }),
        ))
        .expect("invalid cursor response");
    assert_eq!(invalid.error.expect("cursor error").code, "SDK_RUNTIME_INVALID_CURSOR");
}

// Opt-in peak-allocation diagnostics run separately, in fresh processes. They
// intentionally keep large inventories in peers that need no database refresh.
// RSS/HWM are reported rather than asserted as a portable allocator contract.
#[cfg(target_os = "linux")]
fn issue_657_inventory_daemon() -> RpcDaemon {
    let daemon = RpcDaemon::test_instance();
    for index in 1..=512 {
        let peer = format!("{index:032x}");
        daemon
            .upsert_peer(peer.clone(), 1, Vec::new(), None, None, Some("manual".into()))
            .expect("peer");
        daemon
            .peers
            .lock()
            .expect("peers")
            .get_mut(&peer)
            .expect("peer record")
            .restored_handled_ids = (1..=2050).map(|id| format!("{id:064x}")).collect();
    }
    daemon
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 657 million identifier maintenance selection diagnostic"]
fn diagnose_issue_657_maintenance_selection_allocation() {
    let daemon = issue_657_inventory_daemon();
    let before = issue_657_memory();
    let started = std::time::Instant::now();
    assert!(daemon.select_peer_for_maintenance_sync(now_i64()).expect("selection").is_none());
    println!(
        "{}",
        json!({"probe":"maintenance_selection", "owned_ids":512*2050,
        "before_kib":before, "after_kib":issue_657_memory(),
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.0})
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 657 million identifier peer rotation diagnostic"]
fn diagnose_issue_657_peer_rotation_allocation() {
    let daemon = issue_657_inventory_daemon();
    daemon.propagation_state.lock().expect("propagation").max_peers = Some(1024);
    let before = issue_657_memory();
    let started = std::time::Instant::now();
    assert!(daemon.rotate_low_acceptance_non_static_peers().expect("rotation").is_empty());
    println!(
        "{}",
        json!({"probe":"peer_rotation", "owned_ids":512*2050,
        "before_kib":before, "after_kib":issue_657_memory(),
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.0})
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 657 million identifier paginated presence diagnostic"]
fn diagnose_issue_657_presence_page_allocation() {
    let daemon = issue_657_inventory_daemon();
    let before = issue_657_memory();
    let started = std::time::Instant::now();
    let response = daemon
        .handle_rpc(rpc_request(1, "sdk_identity_presence_list_v2", json!({"limit":1})))
        .expect("presence")
        .result
        .expect("presence result");
    assert_eq!(response["presence_list"]["peers"].as_array().expect("rows").len(), 1);
    println!(
        "{}",
        json!({"probe":"presence_page", "owned_ids":512*2050,
        "before_kib":before, "after_kib":issue_657_memory(),
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.0})
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit, bounded production-shaped issue 657 investigation"]
fn diagnose_issue_657_live_shaped_event_retention() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x7a);
    // The reported largest received queue. No production data or identities.
    let count = 27_542_usize;
    for index in 1..=count {
        let entry = PropagationEntryRecord {
            transient_id: format!("{index:064x}"),
            destination: "31".repeat(16),
            payload_hex: "31".repeat(16),
            received_at: now_i64(),
            size_bytes: 16,
            stamp_value: None,
        };
        daemon.store.upsert_propagation_entry(&entry).expect("seed tiny payload");
        daemon
            .store
            .mark_peer_received_propagation(&peer, &entry.transient_id)
            .expect("seed received bookkeeping");
    }
    daemon.event_queue.lock().expect("legacy queue").clear();
    daemon.sdk_event_log.lock().expect("event log").clear();
    let record = daemon.peers.lock().expect("peers").get(&peer).expect("peer").clone();
    println!(
        "{}",
        json!({"stage":"seeded", "received_ids":count,
        "memory_kib":issue_657_memory()})
    );
    let start = std::time::Instant::now();
    for index in 1..=384_u64 {
        drop(daemon.postponed_peer_sync_response(index, &record, now_i64(), "backoff", None, None));
        if [1, 32, 64, 128, 256, 384].contains(&index) {
            let (len, serialized_bytes, inventory_ids) = {
                let log = daemon.sdk_event_log.lock().expect("event log");
                let event = &log.back().expect("last event").event;
                (
                    log.len(),
                    serde_json::to_vec(event).expect("serialize one event").len(),
                    event.payload["messages"]["handled_ids"].as_array().map_or(0, Vec::len),
                )
            };
            println!(
                "{}",
                json!({"stage":"retained", "published":index, "sdk_events":len,
                "last_event_serialized_bytes":serialized_bytes,
                "last_event_inventory_ids":inventory_ids,
                "elapsed_seconds":start.elapsed().as_secs_f64(),
                "memory_kib":issue_657_memory()})
            );
            assert_eq!(len, index as usize);
            assert_eq!(inventory_ids, 0);
            assert!(serialized_bytes < 8192, "summary must fit the smallest SDK event budget");
        }
        // Bound local diagnostic resource use independently of daemon policy.
        if index % 16 == 0 && issue_657_memory()["VmRSS"].as_u64().expect("RSS") > 1_600_000 {
            println!("diagnostic RSS budget reached after {index} publications");
            break;
        }
    }
    for id in 1..=3 {
        let before = daemon.sdk_event_log.lock().expect("event log").len();
        let result = daemon
            .handle_sdk_poll_events_v2(rpc_request(
                10_000 + id,
                "sdk_poll_events_v2",
                json!({"cursor":null, "max":1}),
            ))
            .expect("poll call");
        let after = daemon.sdk_event_log.lock().expect("event log").len();
        assert!(result.error.is_none(), "peer sync history must not poison the poll cursor");
        assert!(result.result.as_ref().is_some_and(|value| value.get("next_cursor").is_some()));
        println!(
            "{}",
            json!({"stage":"poll", "attempt":id,
            "error_code":result.error.map(|error| error.code),
            "has_next_cursor":result.result.as_ref().is_some_and(|value| value.get("next_cursor").is_some()),
            "retained_before":before, "retained_after":after,
            "memory_kib":issue_657_memory()})
        );
        assert_eq!(before, after);
    }
    daemon.event_queue.lock().expect("legacy queue").clear();
    daemon.sdk_event_log.lock().expect("event log").clear();
    println!("{}", json!({"stage":"cleared", "memory_kib":issue_657_memory()}));
}
