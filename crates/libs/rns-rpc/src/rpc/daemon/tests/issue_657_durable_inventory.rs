#[test]
fn issue_657_legacy_inventory_is_released_after_committed_import_and_never_refilled() {
    let daemon = RpcDaemon::test_instance();
    let peer = "issue657-durable-inventory";
    let ids = (1..=256).map(|index| {
        let entry = issue_657_entry(index, 16);
        daemon.store.upsert_propagation_entry(&entry).expect("payload");
        entry.transient_id
    }).collect::<Vec<_>>();
    daemon.peers.lock().expect("peers").insert(peer.into(),
        restored_peer_record(peer, ids[..64].to_vec(), ids[64..].to_vec()));
    assert!(daemon.resource_usage_snapshot().expect("before")["peer_inventory"]["owned_buffer_bytes"].as_u64().expect("bytes") > 0);
    daemon.ensure_peer_queue_import(peer).expect("legacy import");
    for id in &ids[64..] {
        daemon.record_peer_received_propagation(peer, id).expect("complete");
    }
    for _ in 0..3 {
        daemon.queue_existing_propagation_for_peer(peer).expect("refill");
        daemon.maintain_propagation_storage().expect("maintenance");
        let result = daemon.handle_rpc(rpc_request(1, "list_peers", json!({}))).expect("inventory").result.expect("result");
        assert_eq!(result["peers"][0]["messages"]["handled_ids"].as_array().expect("handled").len(), ids.len());
        assert!(result["peers"][0]["messages"]["unhandled_ids"].as_array().expect("pending").is_empty());
        assert_eq!(daemon.resource_usage_snapshot().expect("after")["peer_inventory"]["owned_buffer_bytes"], 0);
    }
    let peers = daemon.peers.lock().expect("peers");
    let record = &peers[peer];
    assert_eq!(record.restored_handled_ids.capacity(), 0);
    assert_eq!(record.restored_unhandled_ids.capacity(), 0);
}

#[test]
fn issue_657_failed_import_keeps_legacy_input_and_rolls_back_before_retry() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("messages.db");
    let daemon = RpcDaemon::with_store(MessagesStore::open(&path).expect("store"), "test".into());
    let peer = "issue657-import-retry";
    let first = issue_657_entry(1, 16);
    let second = issue_657_entry(2, 16);
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("payload");
    }
    daemon.peers.lock().expect("peers").insert(peer.into(),
        restored_peer_record(peer, vec![first.transient_id.clone()], vec![second.transient_id.clone()]));
    let connection = rusqlite::Connection::open(&path).expect("connection");
    connection.execute_batch("CREATE TRIGGER fail_import BEFORE INSERT ON propagation_peer_entries
        WHEN NEW.state = 'unhandled' BEGIN SELECT RAISE(ABORT, 'injected import failure'); END;").expect("trigger");
    assert!(daemon.ensure_peer_queue_import(peer).is_err());
    assert!(daemon.store.list_peer_handled_propagation_ids(peer).expect("rollback").is_empty());
    assert!(!daemon.peer_queue_imports.lock().expect("imports").contains(peer));
    {
        let peers = daemon.peers.lock().expect("peers");
        assert_eq!(peers[peer].restored_handled_ids, vec![first.transient_id.clone()]);
        assert_eq!(peers[peer].restored_unhandled_ids, vec![second.transient_id.clone()]);
    }
    connection.execute_batch("DROP TRIGGER fail_import;").expect("remove trigger");
    daemon.ensure_peer_queue_import(peer).expect("retry");
    assert_eq!(daemon.store.list_peer_handled_propagation_ids(peer).expect("handled"), vec![first.transient_id]);
    assert_eq!(daemon.store.list_peer_unhandled_propagation_ids(peer).expect("pending"), vec![second.transient_id]);
    assert_eq!(daemon.resource_usage_snapshot().expect("released")["peer_inventory"]["owned_buffer_bytes"], 0);
}

#[test]
fn issue_657_import_racing_clear_cannot_resurrect_durable_marks() {
    for clear_method in ["clear_peers", "clear_all"] {
        for _ in 0..8 {
            let daemon = Arc::new(RpcDaemon::test_instance());
            let peer = "issue657-clear-race";
            let entry = issue_657_entry(1, 16);
            daemon.store.upsert_propagation_entry(&entry).expect("payload");
            daemon.peers.lock().expect("peers").insert(peer.into(),
                restored_peer_record(peer, vec![], vec![entry.transient_id]));
            let barrier = Arc::new(std::sync::Barrier::new(3));
            let importer = {
                let daemon = Arc::clone(&daemon);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || { barrier.wait(); daemon.ensure_peer_queue_import(peer).expect("import") })
            };
            let clearer = {
                let daemon = Arc::clone(&daemon);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || { barrier.wait(); daemon.handle_rpc(rpc_request(1, clear_method, json!({}))).expect("clear") })
            };
            barrier.wait();
            importer.join().expect("import worker");
            clearer.join().expect("clear worker");
            assert!(daemon.peers.lock().expect("peers").is_empty());
            assert!(daemon.peer_queue_imports.lock().expect("imports").is_empty());
            assert!(daemon.store.list_peer_unhandled_propagation_ids(peer).expect("marks").is_empty());
        }
    }
}

#[test]
fn issue_657_concurrent_first_import_completion_and_prune_preserve_completed_state() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("messages.db");
    let daemon = Arc::new(RpcDaemon::with_store(
        MessagesStore::open(&path).expect("store"), "test".into()));
    let peer = "issue657-first-import-race";
    let completed = issue_657_entry(1, 16);
    let pending = issue_657_entry(2, 16);
    let expired = issue_657_entry(3, 16);
    for entry in [&completed, &pending, &expired] {
        daemon.store.upsert_propagation_entry(entry).expect("payload");
    }
    daemon.peers.lock().expect("peers").insert(peer.into(),
        restored_peer_record(peer, vec![], vec![completed.transient_id.clone(), pending.transient_id.clone()]));
    // A real expired durable mark must be deleted while first import and
    // completion contend. Fresh-only inputs would never exercise pruning.
    daemon.store.mark_peer_unhandled_propagation(peer, &expired.transient_id).expect("expired mark");
    rusqlite::Connection::open(&path).expect("connection").execute(
        "UPDATE propagation_peer_entries SET updated_at = 0 WHERE transient_id = ?1",
        [&expired.transient_id],
    ).expect("expire mark");
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let finisher = {
        let daemon = Arc::clone(&daemon);
        let barrier = Arc::clone(&barrier);
        let id = completed.transient_id.clone();
        std::thread::spawn(move || { barrier.wait(); daemon.record_peer_received_propagation(peer, &id).expect("complete") })
    };
    let maintainer = {
        let daemon = Arc::clone(&daemon);
        let barrier = Arc::clone(&barrier);
        std::thread::spawn(move || { barrier.wait(); daemon.maintain_propagation_storage().expect("maintenance") })
    };
    barrier.wait();
    finisher.join().expect("completion worker");
    assert_eq!(maintainer.join().expect("maintenance worker"), 1);
    assert_eq!(daemon.store.list_peer_handled_propagation_ids(peer).expect("handled"), vec![completed.transient_id]);
    assert_eq!(daemon.store.list_peer_unhandled_propagation_ids(peer).expect("pending"), vec![pending.transient_id]);
    assert_eq!(daemon.resource_usage_snapshot().expect("released")["peer_inventory"]["owned_buffer_bytes"], 0);
}
