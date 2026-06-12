#[test]
fn persistent_peer_sync_reports_last_batch_transfer_rate_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x50);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 2;
        record.propagation_sync_limit = Some((24 + 30 + 16 + 1) as u32);
    }

    let first = PropagationEntryRecord {
        transient_id: "c5".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "19".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "c6".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "1a".repeat(30),
        received_at: 1_700_000_609,
        size_bytes: 30,
        stamp_value: None,
    };
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(58, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("persistent peer sync")
        .result
        .expect("peer sync result");
    let first_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x19; 20]])).expect("pack first resource").len();
    let second_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x1a; 30]])).expect("pack second resource").len();
    assert_ne!(
        first_resource_bytes, second_resource_bytes,
        "test needs distinct batch sizes"
    );
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(
        result["tx_bytes"].as_u64(),
        Some((first_resource_bytes + second_resource_bytes) as u64)
    );
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(second_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(second_resource_bytes as u64));

    let row = daemon
        .handle_rpc(RpcRequest { id: 59, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result")["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .cloned()
        .expect("peer row");
    assert_eq!(
        row["tx_bytes"].as_u64(),
        Some((first_resource_bytes + second_resource_bytes) as u64)
    );
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(second_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(second_resource_bytes as u64));
}

#[test]
fn lazy_peer_sync_keeps_one_completed_batch_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4f);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 1;
        record.propagation_sync_limit = Some((24 + 20 + 16 + 1) as u32);
    }

    let first = PropagationEntryRecord {
        transient_id: "c3".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "17".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "c4".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_609,
        size_bytes: 20,
        stamp_value: None,
    };
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("lazy peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_strategy"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"].as_array().expect("transferred ids"),
        &[json!(second.transient_id.as_str())]
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![first]);
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![second.transient_id]
    );
}

#[test]
fn peer_sync_skips_entry_at_exact_sync_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(56, "peer_sync", json!({ "peer": "peer-sync-equal-budget" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-sync-equal-budget").expect("peer record");
        peer.propagation_sync_limit = Some((24 + 20 + 16) as u32);
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
        peer.peering_key_value = Some(1);
    }

    let entry = PropagationEntryRecord {
        transient_id: "b3".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(20),
        received_at: 1_700_000_609,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-sync-equal-budget", entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": "peer-sync-equal-budget" })))
        .expect("budgeted peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["offered_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(entry.transient_id.as_str())]
    );

    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-sync-equal-budget")
            .expect("handled ids")
            .is_empty()
    );
    let pending = daemon
        .store
        .list_peer_unhandled_propagation("peer-sync-equal-budget")
        .expect("pending propagation");
    assert_eq!(pending, vec![entry]);
}

#[test]
fn peer_sync_applies_python_per_message_overhead_for_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x45);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some((24 + 40 + 16 + 1) as u32);
    }

    let entry = PropagationEntryRecord {
        transient_id: "b4".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(40),
        received_at: 1_700_000_610,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(
            57,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("budgeted peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![entry.transient_id]);
}

#[test]
fn peer_sync_keeps_transfer_limit_separate_from_missing_sync_limit_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x46);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 1;
        record.propagation_transfer_limit = Some((24 + 20 + 32 + 16 + 1) as u32);
        record.propagation_sync_limit = None;
    }

    let small = PropagationEntryRecord {
        transient_id: "c1".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(20),
        received_at: 1_700_000_610,
        size_bytes: 20,
        stamp_value: None,
    };
    let large = PropagationEntryRecord {
        transient_id: "c2".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(40),
        received_at: 1_700_000_611,
        size_bytes: 40,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&small).expect("store small entry");
    daemon.store.upsert_propagation_entry(&large).expect("store large entry");
    for entry in [&small, &large] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    daemon
        .handle_rpc(rpc_request(
            59,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("budgeted peer sync");

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![small.transient_id, large.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert!(pending.is_empty());
}

#[test]
fn peer_sync_restored_python_transfer_limit_synthesizes_sync_limit_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-transfer-only";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_001_010,
        "peer_type": "manual",
        "sync_strategy": 1,
        "propagation_transfer_limit": 0.07,
        "propagation_stamp_cost": 1,
        "stamp_cost_flexibility": 1,
        "peering_cost": 1,
        "peering_key": [null, 1],
    }))
    .expect("deserialize transfer-only Python peer");
    assert_eq!(record.propagation_transfer_limit, Some(70));
    assert_eq!(record.propagation_sync_limit, Some(70));
    daemon.peers.lock().expect("peers mutex poisoned").insert(peer.to_string(), record);

    let first = PropagationEntryRecord {
        transient_id: "c8".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(10),
        received_at: 1_700_000_615,
        size_bytes: 10,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "c9".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(10),
        received_at: 1_700_000_616,
        size_bytes: 10,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&first).expect("store first entry");
    daemon.store.upsert_propagation_entry(&second).expect("store second entry");
    for entry in [&first, &second] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(60, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(70));

    let handled = daemon.store.list_peer_handled_propagation_ids(peer).expect("handled ids");
    assert_eq!(handled, vec![second.transient_id]);
    let pending = daemon.store.list_peer_unhandled_propagation(peer).expect("pending");
    assert_eq!(pending, vec![first]);
}

#[test]
fn peer_sync_restored_python_fractional_sync_limit_truncates_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-fractional-sync";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_001_011,
        "peer_type": "manual",
        "sync_strategy": 1,
        "propagation_transfer_limit": 1.0,
        "propagation_sync_limit": 0.07,
        "propagation_stamp_cost": 1,
        "stamp_cost_flexibility": 1,
        "peering_cost": 1,
        "peering_key": [null, 1],
    }))
    .expect("deserialize fractional-sync Python peer");
    daemon.peers.lock().expect("peers mutex poisoned").insert(peer.to_string(), record);

    let first = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(10),
        received_at: 1_700_000_617,
        size_bytes: 10,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "d2".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(10),
        received_at: 1_700_000_618,
        size_bytes: 10,
        stamp_value: None,
    };
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(61, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation_sync_limit"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(2));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(2));

    assert!(
        daemon.store.list_peer_handled_propagation_ids(peer).expect("handled ids").is_empty()
    );
    let pending = daemon.store.list_peer_unhandled_propagation(peer).expect("pending");
    assert_eq!(pending, vec![first, second]);
}
