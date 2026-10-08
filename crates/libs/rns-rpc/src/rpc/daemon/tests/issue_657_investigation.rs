// Correctness regressions and opt-in, fresh-process Linux memory qualification.
fn issue_657_entry(index: usize, payload_bytes: usize) -> PropagationEntryRecord {
    PropagationEntryRecord {
        transient_id: format!("{index:064x}"),
        destination: "31".repeat(16),
        payload_hex: "31".repeat(payload_bytes),
        received_at: now_i64().saturating_add(index as i64),
        size_bytes: payload_bytes as u64,
        stamp_value: None,
    }
}

#[test]
fn issue_657_replenishment_respects_existing_pending_capacity() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x69);
    for index in 1..=2050 {
        daemon.store.upsert_propagation_entry(&issue_657_entry(index, 16)).expect("seed");
    }
    let mut counts = Vec::new();
    for _ in 0..3 {
        daemon.queue_existing_propagation_for_peer(&peer).expect("queue");
        counts.push(daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("ids").len());
    }
    let pruned = daemon.maintain_propagation_storage().expect("maintain");
    let after_maintenance =
        daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("ids").len();
    daemon.queue_existing_propagation_for_peer(&peer).expect("queue again");
    let after_replenishment =
        daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("ids").len();
    println!("issue657 queue_counts={counts:?} pruned={pruned} after_maintenance={after_maintenance} after_replenishment={after_replenishment}");
    assert_eq!(counts, vec![1024, 1024, 1024]);
    assert_eq!(pruned, 0);
    assert_eq!(after_maintenance, 1024);
    assert_eq!(after_replenishment, 1024);
}

#[test]
fn issue_657_stale_snapshot_cannot_recreate_pruned_marks() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x6c);
    daemon.propagation_state.lock().expect("state").peer_entry_limit_per_peer = 1;
    for index in 1..=2 {
        let entry = issue_657_entry(index, 16);
        daemon.store.upsert_propagation_entry(&entry).expect("seed");
        daemon.store.mark_peer_unhandled_propagation(&peer, &entry.transient_id).expect("mark");
    }
    daemon.peers.lock().expect("peers").get_mut(&peer).expect("peer").restored_unhandled_ids =
        (1..=2).map(|index| issue_657_entry(index, 16).transient_id).collect();
    let stale = daemon.peers.lock().expect("peers").get(&peer).expect("peer").clone();
    assert_eq!(daemon.maintain_propagation_storage().expect("maintain"), 1);
    let after_prune = daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("ids").len();
    // list_peers and peer_sync can hold this clone while maintenance runs.
    daemon.ensure_peer_queue_import(stale.peer.as_str()).expect("replay stale clone");
    let after_replay = daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("ids").len();
    println!("issue657 stale_snapshot_after_prune={after_prune} after_replay={after_replay}");
    assert_eq!(after_prune, 1);
    assert_eq!(after_replay, 1);
}

#[cfg(target_os = "linux")]
fn issue_657_memory() -> serde_json::Value {
    let status = std::fs::read_to_string("/proc/self/status").expect("status");
    let mut values = serde_json::Map::new();
    for field in ["VmRSS:", "VmHWM:", "VmSwap:", "RssAnon:"] {
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix(field))
            .expect("field")
            .split_whitespace()
            .next()
            .expect("value")
            .parse::<u64>()
            .expect("number");
        values.insert(field.trim_end_matches(':').to_string(), json!(value));
    }
    json!(values)
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 657 persistent queue-refresh memory diagnostic"]
fn diagnose_issue_657_refresh_payload_allocation() {
    let temp = tempfile::tempdir().expect("database directory");
    let daemon = RpcDaemon::with_store(
        MessagesStore::open(&temp.path().join("messages.db")).expect("store"),
        "test-identity".into(),
    );
    let peer = make_ready_propagation_peer(&daemon, 0x6a);
    for index in 1..=256 {
        let entry = issue_657_entry(index, 65_536);
        daemon.store.upsert_propagation_entry(&entry).expect("seed");
        daemon.store.mark_peer_unhandled_propagation(&peer, &entry.transient_id).expect("mark");
    }
    let before = issue_657_memory();
    let started = std::time::Instant::now();
    daemon.ensure_peer_queue_import(&peer).expect("import");
    let durable_ids = daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("durable IDs");
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after = issue_657_memory();
    let ids =
        daemon.peers.lock().expect("peers").get(&peer).expect("peer").restored_unhandled_ids.len();
    assert_eq!(ids, 0);
    assert_eq!(durable_ids.len(), 256);
    println!(
        "{}",
        json!({"probe":"issue657_refresh", "rows":durable_ids.len(), "resident_ids":ids,
        "seed_payload_hex_bytes":256*65_536*2, "before_kib":before,
        "after_kib":after, "elapsed_ms":elapsed_ms})
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 657 million identifier fanout-copy diagnostic"]
fn diagnose_issue_657_fanout_copies_queue_inventory() {
    let daemon = RpcDaemon::test_instance();
    for index in 1..=512 {
        let peer = format!("{index:032x}");
        daemon
            .upsert_peer(peer.clone(), now_i64(), Vec::new(), None, None, Some("manual".into()))
            .expect("peer");
        daemon.peers.lock().expect("peers").get_mut(&peer).expect("peer").restored_unhandled_ids =
            (1..=2050).map(|id| format!("{id:064x}")).collect();
    }
    let before = issue_657_memory();
    let started = std::time::Instant::now();
    let selected = daemon.propagation_fanout_peer_ids();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(selected.len(), 512);
    println!(
        "{}",
        json!({"probe":"issue657_fanout", "peers":512, "owned_ids":512*2050,
        "id_content_bytes":512*2050*64, "before_kib":before,
        "after_kib":issue_657_memory(), "elapsed_ms":elapsed_ms})
    );
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "explicit issue 657 peer-sync event retention diagnostic"]
fn diagnose_issue_657_postponed_sync_event_retention() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x6b);
    for index in 1..=1024 {
        let entry = issue_657_entry(index, 16);
        daemon.store.upsert_propagation_entry(&entry).expect("seed");
        daemon.store.mark_peer_received_propagation(&peer, &entry.transient_id).expect("mark");
    }
    daemon.event_queue.lock().expect("legacy queue").clear();
    daemon.sdk_event_log.lock().expect("SDK log").clear();
    let record = daemon.peers.lock().expect("peers").get(&peer).expect("peer").clone();
    println!("issue657 event_before_kib={}", issue_657_memory());
    for index in 1..=1100 {
        drop(daemon.postponed_peer_sync_response(index, &record, now_i64(), "backoff", None, None));
        if [128, 512, 1024, 1100].contains(&index) {
            let log = daemon.sdk_event_log.lock().expect("SDK log");
            let serialized_bytes: usize = log
                .iter()
                .map(|event| serde_json::to_vec(&event.event).expect("serialize").len())
                .sum();
            println!(
                "{}",
                json!({"probe":"issue657_postponed_sync", "published":index,
                "sdk_events":log.len(), "sdk_serialized_bytes":serialized_bytes,
                "memory_kib":issue_657_memory()})
            );
            assert_eq!(log.len(), index.min(1024) as usize);
            assert!(log
                .iter()
                .all(|event| event.event.payload["messages"].get("handled_ids").is_none()));
        }
    }
    daemon.event_queue.lock().expect("legacy queue").clear();
    daemon.sdk_event_log.lock().expect("SDK log").clear();
    println!("issue657 event_cleared_kib={}", issue_657_memory());
}

#[test]
fn issue_657_peer_sync_events_keep_counts_without_queue_or_payload_copies() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x6d);
    let entry = issue_657_entry(1, 16);
    daemon.store.upsert_propagation_entry(&entry).expect("seed");
    daemon.store.mark_peer_received_propagation(&peer, &entry.transient_id).expect("mark");
    let record = daemon.peers.lock().expect("peers").get(&peer).expect("peer").clone();
    let response =
        daemon.postponed_peer_sync_response(1, &record, now_i64(), "backoff", None, None);
    assert_eq!(
        response.result.expect("result")["messages"]["handled_ids"],
        json!([entry.transient_id])
    );
    let event = daemon.sdk_event_log.lock().expect("SDK log").back().expect("event").clone();
    assert_eq!(event.event.payload["messages"]["incoming"], 1);
    assert!(event.event.payload["messages"].get("handled_ids").is_none());
    assert!(event.event.payload["propagation"].get("messages").is_none());
    let event = daemon.push_event(RpcEvent {
        event_type: "peer_sync".into(),
        payload: json!({"propagation": {"transferred": 1, "transferred_ids": [entry.transient_id],
            "messages": [{"payload_hex": entry.payload_hex}]}}),
    });
    assert_eq!(event.payload["propagation"]["transferred"], 1);
    assert!(event.payload["propagation"].get("transferred_ids").is_none());
    assert!(event.payload["propagation"].get("messages").is_none());
}

#[test]
fn issue_657_concurrent_refills_use_remaining_capacity_and_preserve_completed_marks() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let peer = make_ready_propagation_peer(&daemon, 0x6e);
    daemon.propagation_state.lock().expect("state").peer_entry_limit_per_peer = 4;
    for index in 1..=12 {
        daemon.store.upsert_propagation_entry(&issue_657_entry(index, 16)).expect("seed");
    }
    let completed = issue_657_entry(12, 16).transient_id;
    daemon
        .store
        .mark_peer_received_propagation(&peer.to_ascii_uppercase(), &completed)
        .expect("completed");
    let mut workers = Vec::new();
    for index in 0..8 {
        let daemon = daemon.clone();
        let peer = if index % 2 == 0 { peer.to_ascii_uppercase() } else { peer.clone() };
        workers.push(std::thread::spawn(move || {
            daemon.queue_existing_propagation_for_peer(&peer).expect("refill")
        }));
    }
    for worker in workers {
        worker.join().expect("worker");
    }
    let pending = daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("pending");
    assert_eq!(pending.len(), 4);
    assert!(!pending.contains(&completed));
    assert!(daemon
        .store
        .peer_received_propagation_mark_exists(&peer, &completed)
        .expect("received"));
    daemon.store.mark_peer_handled_propagation(&peer, &pending[0]).expect("finish one");
    daemon.queue_existing_propagation_for_peer(&peer).expect("refill one slot");
    let refilled = daemon.store.list_peer_unhandled_propagation_ids(&peer).expect("pending");
    assert_eq!(refilled.len(), 4);
    assert!(!refilled.contains(&pending[0]));
    assert!(!refilled.contains(&completed));
}

#[test]
fn issue_657_peer_reads_do_not_write_after_legacy_import_or_replay_stale_live_cache() {
    let temp = tempfile::tempdir().expect("database directory");
    let daemon = RpcDaemon::with_store(
        MessagesStore::open(&temp.path().join("messages.db")).expect("store"),
        "test-identity".into(),
    );
    let peer = "issue657-restored-peer";
    let first = issue_657_entry(1, 16);
    let second = issue_657_entry(2, 16);
    daemon.store.upsert_propagation_entry(&first).expect("first");
    daemon.store.upsert_propagation_entry(&second).expect("second");
    daemon.peers.lock().expect("peers").insert(
        peer.into(),
        restored_peer_record(
            peer,
            vec![],
            vec![first.transient_id.clone(), second.transient_id.clone()],
        ),
    );
    daemon.propagation_state.lock().expect("state").peer_entry_limit_per_peer = 1;
    assert_eq!(daemon.maintain_propagation_storage().expect("import then prune"), 1);
    // Even an outdated live cache must not regain authority after initial import.
    daemon
        .peers
        .lock()
        .expect("peers")
        .get_mut(peer)
        .expect("peer")
        .restored_unhandled_ids
        .push(first.transient_id);
    let writes = daemon.store.contention_snapshot().write_ops_total;
    for id in 1..=3 {
        let result = daemon
            .handle_rpc(RpcRequest { id, method: "list_peers".into(), params: None })
            .expect("read")
            .result
            .expect("result");
        assert_eq!(result["peers"][0]["messages"]["unhandled"], 1);
    }
    assert_eq!(daemon.store.contention_snapshot().write_ops_total, writes);
    assert_eq!(
        daemon.store.list_peer_unhandled_propagation_ids(peer).expect("pending"),
        vec![second.transient_id]
    );
}

#[test]
fn issue_657_fanout_preserves_static_recent_and_tie_ordering() {
    let daemon = RpcDaemon::test_instance();
    for (peer, seen, kind) in
        [("b", 20, "manual"), ("a", 20, "manual"), ("c", 1, "manual"), ("d", 99, "unpeered")]
    {
        daemon
            .upsert_peer(peer.into(), seen, Vec::new(), None, None, Some(kind.into()))
            .expect("peer");
    }
    let mut state = daemon.propagation_state.lock().expect("state");
    state.max_propagation_peers = 2;
    state.static_peers = vec!["C".into()];
    drop(state);
    assert_eq!(daemon.propagation_fanout_peer_ids(), vec!["c", "a"]);
}

#[test]
fn issue_657_import_tracking_is_released_with_peer_records() {
    let daemon = RpcDaemon::test_instance();
    for peer in ["Issue657-one", "issue657-two", "issue657-three"] {
        daemon
            .upsert_peer(peer.into(), now_i64(), Vec::new(), None, None, Some("manual".into()))
            .expect("peer");
        daemon.ensure_peer_queue_import(peer).expect("import");
    }
    assert_eq!(daemon.peer_queue_imports.lock().expect("imports").len(), 3);
    daemon.unpeer_local_state("ISSUE657-ONE").expect("unpeer");
    assert_eq!(daemon.peer_queue_imports.lock().expect("imports").len(), 2);
    daemon.remove_peer_if_stale_or_expensive("issue657-two", now_i64()).expect("remove stale");
    assert_eq!(daemon.peer_queue_imports.lock().expect("imports").len(), 1);
    daemon.handle_rpc(rpc_request(1, "clear_peers", json!({}))).expect("clear");
    assert!(daemon.peer_queue_imports.lock().expect("imports").is_empty());
}

#[test]
fn issue_657_legacy_import_updates_new_completions_without_renewing_existing_completed_ttl() {
    let temp = tempfile::tempdir().expect("database directory");
    let path = temp.path().join("messages.db");
    let daemon =
        RpcDaemon::with_store(MessagesStore::open(&path).expect("store"), "test-identity".into());
    let peer = "issue657-completion-age";
    let pending = issue_657_entry(1, 16);
    let received = issue_657_entry(2, 16);
    for entry in [&pending, &received] {
        daemon.store.upsert_propagation_entry(entry).expect("seed");
    }
    daemon.store.mark_peer_unhandled_propagation(peer, &pending.transient_id).expect("pending");
    daemon.store.mark_peer_received_propagation(peer, &received.transient_id).expect("received");
    let conn = rusqlite::Connection::open(&path).expect("connection");
    conn.execute("UPDATE propagation_peer_entries SET updated_at = ?1", [now_i64() - 100])
        .expect("age marks");
    daemon.peers.lock().expect("peers").insert(
        peer.into(),
        restored_peer_record(
            peer,
            vec![pending.transient_id.clone(), received.transient_id.clone()],
            vec![],
        ),
    );
    daemon.propagation_state.lock().expect("state").completed_peer_entry_ttl_secs = 50;
    assert_eq!(daemon.maintain_propagation_storage().expect("import then prune"), 1);
    assert_eq!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("handled"),
        vec![pending.transient_id]
    );
    assert!(!daemon
        .store
        .peer_propagation_mark_exists(peer, &received.transient_id)
        .expect("expired"));
}
