#[test]
fn persistent_peer_sync_continues_after_completed_batch_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4e);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 2;
        record.propagation_sync_limit = Some((24 + 20 + 16 + 1) as u32);
    }

    let first = PropagationEntryRecord {
        transient_id: "c1".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "15".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "c2".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "16".repeat(20),
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
        .expect("persistent peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(
        result["propagation"]["transferred_ids"].as_array().expect("transferred ids"),
        &[json!(second.transient_id.as_str()), json!(first.transient_id.as_str())]
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert!(pending.is_empty());
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![first.transient_id, second.transient_id]
    );
}

#[test]
fn persistent_peer_sync_uses_restored_python_float_sync_strategy_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-float-strategy";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_619,
        "alive": true,
        "propagation_sync_limit": 61,
        "propagation_stamp_cost": 0,
        "propagation_stamp_cost_flexibility": 0,
        "peering_cost": 0,
        "peering_key": ["opaque-python-key", 0],
        "sync_strategy": 2.0,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored Python peer with float sync strategy");
    assert_eq!(record.sync_strategy, 2);
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let first = PropagationEntryRecord {
        transient_id: "cb".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "1f".repeat(20),
        received_at: 1_700_000_620,
        size_bytes: 20,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "cc".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "20".repeat(20),
        received_at: 1_700_000_621,
        size_bytes: 20,
        stamp_value: None,
    };
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("store entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer })))
        .expect("persistent restored-float-strategy peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(2));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(second.transient_id.as_str()), json!(first.transient_id.as_str())]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer)
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_restored_python_float_timestamps_drive_transfer_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-float-timestamps";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_622.9,
        "last_sync_attempt": 1_700_000_610.4,
        "next_sync_attempt": 0.0,
        "peering_timebase": 1_700_000_600.8,
        "alive": true,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 0,
        "propagation_stamp_cost_flexibility": 0,
        "peering_cost": 0,
        "peering_key": ["opaque-python-key", 0],
        "sync_strategy": 1,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored Python peer with float timestamps");
    assert_eq!(record.last_seen, 1_700_000_622);
    assert_eq!(record.last_sync_attempt, 1_700_000_610);
    assert_eq!(record.next_sync_attempt, 0);
    assert_eq!(record.peering_timebase, 1_700_000_600);
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let entry = PropagationEntryRecord {
        transient_id: "cd".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "21".repeat(20),
        received_at: 1_700_000_623,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer })))
        .expect("restored-float-timestamp peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["synced"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn peer_sync_restored_python_float_counters_preserve_queue_accounting_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = "peer-restored-float-counters";
    let record: PeerRecord = serde_json::from_value(json!({
        "destination_hash": peer,
        "last_heard": 1_700_000_624,
        "alive": true,
        "offered": 2.0,
        "outgoing": 1.0,
        "incoming": 3.0,
        "rx_bytes": 10.0,
        "tx_bytes": 20.0,
        "propagation_sync_limit": 1,
        "propagation_stamp_cost": 0,
        "propagation_stamp_cost_flexibility": 0,
        "peering_cost": 0,
        "peering_key": ["opaque-python-key", 0],
        "sync_strategy": 1,
        "handled_ids": [],
        "unhandled_ids": [],
    }))
    .expect("deserialize restored Python peer with float counters");
    assert_eq!(record.offered, 2);
    assert_eq!(record.outgoing, 1);
    assert_eq!(record.incoming, 3);
    assert_eq!(record.rx_bytes, 10);
    assert_eq!(record.tx_bytes, 20);
    daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .insert(peer.to_string(), record);

    let entry = PropagationEntryRecord {
        transient_id: "ce".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "22".repeat(20),
        received_at: 1_700_000_625,
        size_bytes: 20,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer, entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(57, "peer_sync", json!({ "peer": peer })))
        .expect("restored-float-counter peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["messages"]["offered"].as_u64(), Some(3));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(2));
    assert_eq!(result["messages"]["incoming"].as_u64(), Some(3));
    assert_eq!(result["rx_bytes"].as_u64(), Some(10));
    assert!(
        result["tx_bytes"].as_u64().is_some_and(|value| value > 20),
        "tx_bytes should include the restored value plus this transfer"
    );
    assert!(
        result["acceptance_rate"]
            .as_f64()
            .is_some_and(|value| (value - (2.0 / 3.0)).abs() < f64::EPSILON)
    );
}

#[test]
fn persistent_peer_sync_keeps_selected_response_skips_for_next_offer_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x52);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 2;
        record.propagation_sync_limit = Some((24 + 20 + 16 + 1) as u32);
    }

    let first = PropagationEntryRecord {
        transient_id: "c7".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "1b".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "c8".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "1c".repeat(20),
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
        .handle_rpc(rpc_request(
            58,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [second.transient_id.as_str()],
            }),
        ))
        .expect("persistent selected peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"].as_array().expect("transferred ids"),
        &[json!(second.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(first.transient_id.as_str())]
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![first]);
}

#[test]
fn persistent_peer_sync_keeps_true_response_skips_for_next_offer_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x53);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 2;
        record.propagation_sync_limit = Some((24 + 20 + 16 + 1) as u32);
    }

    let first = PropagationEntryRecord {
        transient_id: "c9".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "1d".repeat(20),
        received_at: 1_700_000_608,
        size_bytes: 20,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "ca".repeat(32),
        destination: "15".repeat(16),
        payload_hex: "1e".repeat(20),
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
        .handle_rpc(rpc_request(
            58,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": true,
            }),
        ))
        .expect("persistent true peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["sync_strategy"].as_u64(), Some(2));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"].as_array().expect("transferred ids"),
        &[json!(second.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(first.transient_id.as_str())]
    );

    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![first]);
}
