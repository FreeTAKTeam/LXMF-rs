#[test]
fn propagation_peer_maintenance_caps_unknown_speed_pool_like_python() {
    let daemon = RpcDaemon::test_instance();
    let fast_peer = make_ready_propagation_peer(&daemon, 0x5a);
    let slower_peer = make_ready_propagation_peer(&daemon, 0x5b);
    let first_unknown_peer = make_ready_propagation_peer(&daemon, 0x5c);
    let second_unknown_peer = make_ready_propagation_peer(&daemon, 0x5d);
    let third_unknown_peer = make_ready_propagation_peer(&daemon, 0x5e);
    let entry = PropagationEntryRecord {
        transient_id: "da".repeat(32),
        destination: "1c".repeat(16),
        payload_hex: "28".repeat(32),
        received_at: 1_700_000_624,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [
        &fast_peer,
        &slower_peer,
        &first_unknown_peer,
        &second_unknown_peer,
        &third_unknown_peer,
    ] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for (peer, rate) in [
            (fast_peer.as_str(), 2_048.0),
            (slower_peer.as_str(), 1_024.0),
            (first_unknown_peer.as_str(), 0.0),
            (second_unknown_peer.as_str(), 0.0),
            (third_unknown_peer.as_str(), 0.0),
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.alive = true;
            record.last_seen = 1_700_000_624;
            record.last_sync_attempt = record.last_seen.saturating_sub(1);
            record.next_sync_attempt = 0;
            record.sync_transfer_rate = rate;
        }
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_624)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(fast_peer.as_str()));
}

#[test]
fn propagation_peer_maintenance_skips_waiting_peer_in_backoff_like_python() {
    let daemon = RpcDaemon::test_instance();
    let backed_off_peer = make_ready_propagation_peer(&daemon, 0x60);
    let due_peer = make_ready_propagation_peer(&daemon, 0x61);
    let entry = PropagationEntryRecord {
        transient_id: "dc".repeat(32),
        destination: "1d".repeat(16),
        payload_hex: "29".repeat(32),
        received_at: 1_700_000_626,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [&backed_off_peer, &due_peer] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }
    {
        let timestamp = 1_700_000_626;
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let backed_off = peers.get_mut(backed_off_peer.as_str()).expect("backed-off peer");
        backed_off.alive = true;
        backed_off.last_seen = timestamp;
        backed_off.last_sync_attempt = timestamp.saturating_sub(1);
        backed_off.next_sync_attempt = timestamp.saturating_add(12 * 60);
        backed_off.sync_transfer_rate = 2_048.0;

        let due = peers.get_mut(due_peer.as_str()).expect("due peer");
        due.alive = true;
        due.last_seen = timestamp;
        due.last_sync_attempt = timestamp.saturating_sub(1);
        due.next_sync_attempt = 0;
        due.sync_transfer_rate = 1_024.0;
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_626)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(due_peer.as_str()));
}

#[test]
fn propagation_peer_maintenance_skips_unresponsive_peer_at_backoff_boundary_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x62);
    let entry = PropagationEntryRecord {
        transient_id: "dd".repeat(32),
        destination: "1e".repeat(16),
        payload_hex: "2a".repeat(32),
        received_at: 1_700_000_627,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    {
        let timestamp = 1_700_000_627;
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = false;
        record.last_seen = timestamp;
        record.last_sync_attempt = timestamp.saturating_sub(1);
        record.next_sync_attempt = timestamp;
        record.sync_transfer_rate = 0.0;
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_627)
        .expect("select maintenance sync peer");

    assert!(selected.is_none(), "peer at exact retry boundary should stay in backoff");
}

#[test]
fn peer_sync_backoff_boundary_remains_postponed_like_python() {
    assert!(dispatch_legacy_messages::peer_sync_backoff_active(99, 100));
    assert!(dispatch_legacy_messages::peer_sync_backoff_active(100, 100));
    assert!(!dispatch_legacy_messages::peer_sync_backoff_active(101, 100));
    assert!(!dispatch_legacy_messages::peer_sync_backoff_active(100, 0));
}

#[test]
fn propagation_peer_maintenance_unresponsive_pool_does_not_starve_later_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    let first_peer = make_ready_propagation_peer(&daemon, 0x57);
    let second_peer = make_ready_propagation_peer(&daemon, 0x58);
    let third_peer = make_ready_propagation_peer(&daemon, 0x59);
    let entry = PropagationEntryRecord {
        transient_id: "d8".repeat(32),
        destination: "1b".repeat(16),
        payload_hex: "27".repeat(32),
        received_at: 1_700_000_621,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [&first_peer, &second_peer, &third_peer] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark unhandled");
    }
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for peer in [&first_peer, &second_peer, &third_peer] {
            let record = peers.get_mut(peer.as_str()).expect("peer record");
            record.alive = false;
            record.last_seen = 1_700_000_621;
            record.last_sync_attempt = record.last_seen.saturating_sub(1);
            record.next_sync_attempt = 0;
        }
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_623)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(second_peer.as_str()));
}

#[test]
fn propagation_peer_maintenance_does_not_sync_unreachable_static_peer_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            53,
            "propagation_enable",
            json!({
                "enabled": true,
                "static_peers": ["peer-static-unreachable-sync-skip"],
            }),
        ))
        .expect("enable propagation");

    let entry = PropagationEntryRecord {
        transient_id: "d6".repeat(32),
        destination: "19".repeat(16),
        payload_hex: "25".repeat(32),
        received_at: 1_700_000_619,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-static-unreachable-sync-skip",
            entry.transient_id.as_str(),
        )
        .expect("mark unhandled");
    daemon
        .accept_announce_with_metadata(
            "peer-static-unreachable-sync-skip".to_string(),
            1_700_000_619,
            Some("Static Unreachable".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(1),
            Some(Some(0)),
            Some(Some(1)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept static peer announce");
    let stale_last_seen = now_i64() - (14 * 24 * 60 * 60) - 1;
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers
            .get_mut("peer-static-unreachable-sync-skip")
            .expect("static peer");
        record.alive = false;
        record.last_seen = stale_last_seen;
        record.next_sync_attempt = 0;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(54, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["synced_peer"].as_str(), None);
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-static-unreachable-sync-skip")
            .expect("list unhandled"),
        vec![entry]
    );
    assert!(
        std::iter::from_fn(|| daemon.take_event()).all(|event| event.event_type != "peer_sync")
    );
}

#[test]
fn stale_announce_does_not_regress_propagation_peer_state() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            47,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_200,
            Some("New Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(5),
            Some(Some(2)),
            Some(Some(6)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept fresh announce");
    daemon
        .accept_announce_with_metadata(
            "peer-auto".to_string(),
            1_700_000_150,
            Some("Old Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(2),
            Some(Some(0)),
            Some(Some(3)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept stale announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 48, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["name"].as_str(), Some("New Peer"));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_200));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(5));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(6));

    let announces = daemon
        .handle_rpc(RpcRequest { id: 49, method: "list_announces".to_string(), params: None })
        .expect("list announces")
        .result
        .expect("list announces result");
    let rows = announces["announces"].as_array().expect("announce rows");
    assert_eq!(rows.first().and_then(|row| row["timestamp"].as_i64()), Some(1_700_000_200));
    assert_eq!(rows.get(1).and_then(|row| row["timestamp"].as_i64()), Some(1_700_000_150));
}

#[test]
fn equal_timebase_announce_does_not_refresh_propagation_peer_state_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            49,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto-equal-timebase".to_string(),
            1_700_000_210,
            Some("Equal Timebase Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(5),
            Some(Some(2)),
            Some(Some(6)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept initial announce");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-equal-timebase".to_string(),
            1_700_000_210,
            Some("Equal Timebase Peer".to_string()),
            Some("announce".to_string()),
            None,
            Some(vec!["propagation".to_string()]),
            None,
            None,
            None,
            Some(2),
            Some(Some(0)),
            Some(Some(3)),
            None,
            Some(1),
            None,
            None,
            None,
            None,
        )
        .expect("accept equal-timebase announce");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 50, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_210));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(5));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(6));
}
