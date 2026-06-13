#[test]
fn peer_sync_orders_offers_by_python_weight_before_sync_limit() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x49);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 1;
        record.propagation_sync_limit = Some(152);
    }

    let older_large = PropagationEntryRecord {
        transient_id: "c4".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(80),
        received_at: 1_700_000_612,
        size_bytes: 80,
        stamp_value: None,
    };
    let newer_small = PropagationEntryRecord {
        transient_id: "c5".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(20),
        received_at: 1_700_000_613,
        size_bytes: 20,
        stamp_value: None,
    };
    for entry in [&older_large, &newer_small] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            63,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(newer_small.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(older_large.transient_id.as_str())]
    );

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![newer_small.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![older_large]);
}

#[test]
fn peer_sync_prioritised_destinations_reduce_offer_weight_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4a);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.sync_strategy = 1;
        record.propagation_sync_limit = Some(152);
    }
    daemon
        .handle_rpc(rpc_request(
            63,
            "set_delivery_policy",
            json!({
                "prioritised_destinations": ["17".repeat(16)],
            }),
        ))
        .expect("set delivery policy");

    let prioritised_large = PropagationEntryRecord {
        transient_id: "c6".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(80),
        received_at: 1_700_000_614,
        size_bytes: 80,
        stamp_value: None,
    };
    let normal_small = PropagationEntryRecord {
        transient_id: "c7".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "18".repeat(20),
        received_at: 1_700_000_615,
        size_bytes: 20,
        stamp_value: None,
    };
    for entry in [&prioritised_large, &normal_small] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }

    let result = daemon
        .handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(prioritised_large.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(normal_small.transient_id.as_str())]
    );

    let handled = daemon
        .store
        .list_peer_handled_propagation_ids(peer.as_str())
        .expect("handled ids");
    assert_eq!(handled, vec![prioritised_large.transient_id]);
    let pending = daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("pending propagation");
    assert_eq!(pending, vec![normal_small]);
}

#[test]
fn peer_sync_reports_propagation_transfer_accounting() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4a);
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some((24 + 20 + 32 + 16 + 1) as u32);
    }

    let small = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(20),
        received_at: 1_700_000_612,
        size_bytes: 20,
        stamp_value: None,
    };
    let large = PropagationEntryRecord {
        transient_id: "d2".repeat(32),
        destination: "17".repeat(16),
        payload_hex: "17".repeat(100),
        received_at: 1_700_000_613,
        size_bytes: 100,
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

    let result = daemon
        .handle_rpc(rpc_request(
            61,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let expected_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x17; 20]])).expect("pack sync resource").len();
    assert!(
        expected_resource_bytes > small.size_bytes as usize,
        "resource accounting should include Python LXMPeer msgpack envelope overhead"
    );
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(20));
    assert_eq!(result["propagation"]["offered_bytes"].as_u64(), Some(20));
    assert_eq!(result["propagation"]["remaining"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["remaining_bytes"].as_u64(), Some(100));
    assert_eq!(
        result["propagation"]["sync_limit"].as_u64(),
        Some((24 + 20 + 32 + 16 + 1) as u64)
    );
    assert_eq!(result["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(1.0));
    assert_eq!(
        result["propagation"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(small.transient_id.as_str())]
    );
    assert_eq!(
        result["propagation"]["skipped_ids"].as_array().expect("skipped ids"),
        &[json!(large.transient_id.as_str())]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("peer sync event");
    assert_eq!(event.payload["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["skipped"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["bytes"].as_u64(), Some(20));
    assert_eq!(event.payload["propagation"]["offered_bytes"].as_u64(), Some(20));
    assert_eq!(event.payload["propagation"]["remaining"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation"]["remaining_bytes"].as_u64(), Some(100));
    assert_eq!(event.payload["synced"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["synced"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(event.payload["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(event.payload["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["acceptance_rate"].as_f64(), Some(1.0));
    assert_eq!(
        event.payload["propagation"]["handled_ids"].as_array().expect("event handled ids"),
        &[json!(small.transient_id.as_str())]
    );
    assert_eq!(
        event.payload["propagation"]["skipped_ids"].as_array().expect("event skipped ids"),
        &[json!(large.transient_id.as_str())]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 62, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["tx_bytes"].as_u64(), Some(expected_resource_bytes as u64));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(row["messages"]["outgoing"].as_u64(), Some(1));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["acceptance_rate"].as_f64(), Some(1.0));
}

#[test]
fn peer_sync_updates_transfer_rate_from_transferred_bytes() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4b);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.propagation_sync_limit = Some(1_000);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d7".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "25".repeat(40),
        received_at: 1_700_000_619,
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
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    let expected_resource_bytes =
        rmp_serde::to_vec(&(1.0_f64, vec![vec![0x25; 40]])).expect("pack sync resource").len();
    assert_eq!(result["propagation"]["bytes"].as_u64(), Some(40));
    assert_eq!(result["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(result["str"].as_u64(), Some(expected_resource_bytes as u64));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 65, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["sync_transfer_rate"].as_f64(), Some(expected_resource_bytes as f64));
    assert_eq!(row["str"].as_u64(), Some(expected_resource_bytes as u64));
}

#[test]
fn peer_sync_no_transfer_preserves_last_heard_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x4d);
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = false;
        record.last_seen = 7;
        record.seen_count = 3;
        record.propagation_sync_limit = Some(1_000);
    }

    let entry = PropagationEntryRecord {
        transient_id: "db".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "25".repeat(40),
        received_at: 1_700_000_619,
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
            64,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [],
                "transfer_limit_kb": 1.0,
            }),
        ))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["handled"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    let last_sync_attempt = result["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 7);
    assert_eq!(result["last_heard"].as_i64(), Some(7));
    assert_eq!(result["seen_count"].as_u64(), Some(3));
    assert_eq!(result["alive"].as_bool(), Some(false));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 65, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(peer.as_str()))
        .expect("peer row");
    assert_eq!(row["last_heard"].as_i64(), Some(7));
    assert_eq!(row["seen_count"].as_u64(), Some(3));
    assert_eq!(row["alive"].as_bool(), Some(false));
}
