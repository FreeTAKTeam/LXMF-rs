#[test]
fn propagation_peer_maintenance_rotation_replays_restored_queue_before_drop_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            45,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "max_peers": 3,
            }),
        ))
        .expect("enable propagation");

    for (peer, timestamp) in [
        ("peer-rotation-restored-low", 1_700_000_610),
        ("peer-rotation-restored-keep-a", 1_700_000_611),
        ("peer-rotation-restored-keep-b", 1_700_000_612),
    ] {
        daemon
            .accept_announce_with_metadata(
                peer.to_string(),
                timestamp,
                Some(peer.to_string()),
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
            .expect("accept autopeer announce");
    }

    let entry = PropagationEntryRecord {
        transient_id: "ab".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_613,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    let recent = now_i64();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for peer in [
            "peer-rotation-restored-low",
            "peer-rotation-restored-keep-a",
            "peer-rotation-restored-keep-b",
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.last_seen = recent;
            record.alive = true;
            record.last_sync_attempt = recent - 1;
            record.offered = 10;
            record.outgoing = 10;
        }
        let low = peers.get_mut("peer-rotation-restored-low").expect("low-rate peer");
        low.outgoing = 0;
        low.restored_unhandled_ids.push(entry.transient_id.clone());
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(46, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["rotated"].as_u64(), Some(0));
    assert_eq!(result["rotated_peers"].as_array().expect("rotated peers"), &[] as &[JsonValue]);
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-rotation-restored-low")
            .expect("pending propagation"),
        vec![entry]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 47, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().any(|row| {
        row["peer"].as_str() == Some("peer-rotation-restored-low")
            && row["peer_type"].as_str() == Some("auto")
    }));
}

#[test]
fn propagation_peer_maintenance_rotates_low_acceptance_non_static_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            47,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "max_peers": 3,
                "static_peers": ["peer-rotation-static"],
            }),
        ))
        .expect("enable propagation");

    daemon
        .handle_rpc(rpc_request(48, "peer_sync", json!({ "peer": "peer-rotation-manual-low" })))
        .expect("create manual peer");
    daemon
        .handle_rpc(rpc_request(49, "peer_sync", json!({ "peer": "peer-rotation-static" })))
        .expect("create static peer");
    daemon
        .accept_announce_with_metadata(
            "peer-rotation-auto-keep".to_string(),
            1_700_000_613,
            Some("peer-rotation-auto-keep".to_string()),
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
        .expect("accept autopeer announce");

    let recent = now_i64();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for (peer, outgoing) in [
            ("peer-rotation-manual-low", 0),
            ("peer-rotation-static", 10),
            ("peer-rotation-auto-keep", 10),
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.last_seen = recent;
            record.alive = true;
            record.last_sync_attempt = recent - 1;
            record.offered = 10;
            record.outgoing = outgoing;
        }
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(50, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["rotated"].as_u64(), Some(1));
    assert_eq!(
        result["rotated_peers"].as_array().expect("rotated peers"),
        &[json!("peer-rotation-manual-low")]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(
        rows.iter().all(|row| row["peer"].as_str() != Some("peer-rotation-manual-low"))
    );
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-static")));
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-auto-keep")));

    let event = std::iter::from_fn(|| daemon.take_event())
        .find(|event| event.event_type == "peer_unpeer")
        .expect("rotation unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-rotation-manual-low"));
    assert_eq!(event.payload["reason"].as_str(), Some("peer_rotation"));
}

#[test]
fn propagation_peer_maintenance_syncs_one_waiting_peer_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x53);
    let entry = PropagationEntryRecord {
        transient_id: "d5".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "24".repeat(32),
        received_at: 1_700_000_618,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = true;
        record.last_seen = now_i64();
        record.last_sync_attempt = record.last_seen.saturating_sub(1);
        record.next_sync_attempt = 0;
        record.sync_transfer_rate = 1024.0;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(52, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["rotated"].as_u64(), Some(0));
    assert_eq!(result["synced_peer"].as_str(), Some(peer.as_str()));
    assert_eq!(result["peer_sync"]["peer"].as_str(), Some(peer.as_str()));
    assert_eq!(result["peer_sync"]["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["peer_sync"]["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );

    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("list unhandled")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![entry.transient_id]
    );

    let event = std::iter::from_fn(|| daemon.take_event())
        .find(|event| event.event_type == "peer_sync")
        .expect("maintenance peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some(peer.as_str()));
    assert_eq!(event.payload["propagation"]["transferred"].as_u64(), Some(1));
}

#[test]
fn propagation_peer_maintenance_prunes_expired_local_processed_marks_like_python() {
    let (daemon, _) = ready_propagation_peer_daemon(0x57);
    let expired = "e7".repeat(32);
    let fresh = "f8".repeat(32);
    let now = now_i64();
    daemon
        .store
        .mark_local_propagation_processed_at(
            expired.as_str(),
            now - crate::storage::messages::LXMF_LOCAL_TRANSIENT_CACHE_EXPIRY_SECS - 1,
        )
        .expect("insert expired mark");
    daemon
        .store
        .mark_local_propagation_processed_at(fresh.as_str(), now - 1)
        .expect("insert fresh mark");

    let result = daemon
        .handle_rpc(rpc_request(57, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["pruned_local_processed"].as_u64(), Some(1));
    assert_eq!(
        result["pruned_local_processed_ids"].as_array().expect("pruned ids"),
        &[json!(expired)]
    );
    assert!(!daemon
        .store
        .local_propagation_processed_mark_exists(expired.as_str())
        .expect("expired mark pruned"));
    assert!(daemon
        .store
        .local_propagation_processed_mark_exists(fresh.as_str())
        .expect("fresh mark retained"));
}

#[test]
fn propagation_peer_maintenance_selection_claims_peer_before_sync_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x64);
    let entry = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "20".repeat(16),
        payload_hex: "2c".repeat(32),
        received_at: 1_700_000_629,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = true;
        record.last_seen = 1_700_000_629;
        record.last_sync_attempt = 1_700_000_600;
        record.next_sync_attempt = 0;
        record.sync_backoff = 0;
        record.sync_transfer_rate = 1024.0;
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_629)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(peer.as_str()));
    let peers = daemon.peers.lock().expect("peers mutex poisoned");
    let record = peers.get(peer.as_str()).expect("peer record");
    assert_eq!(record.last_sync_attempt, 1_700_000_629);
    assert_eq!(record.sync_backoff, 12 * 60);
    assert_eq!(record.next_sync_attempt, 1_700_000_629 + 12 * 60);
}

#[test]
fn propagation_peer_maintenance_replays_restored_unhandled_queue_like_python() {
    let (daemon, peer) = ready_propagation_peer_daemon(0x63);
    let entry = PropagationEntryRecord {
        transient_id: "df".repeat(32),
        destination: "1f".repeat(16),
        payload_hex: "2b".repeat(32),
        received_at: 1_700_000_628,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    {
        let timestamp = now_i64();
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.alive = true;
        record.last_seen = timestamp;
        record.last_sync_attempt = timestamp.saturating_sub(1);
        record.next_sync_attempt = 0;
        record.sync_transfer_rate = 1024.0;
        record.restored_unhandled_ids.push(entry.transient_id.clone());
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(53, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["synced_peer"].as_str(), Some(peer.as_str()));
    assert_eq!(result["peer_sync"]["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["peer_sync"]["propagation"]["transferred_ids"]
            .as_array()
            .expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(daemon
        .store
        .list_peer_unhandled_propagation(peer.as_str())
        .expect("list unhandled")
        .is_empty());
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![entry.transient_id]
    );
}

#[test]
fn propagation_peer_maintenance_candidate_pool_includes_unknown_speed_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    let fast_peer = make_ready_propagation_peer(&daemon, 0x54);
    let slower_peer = make_ready_propagation_peer(&daemon, 0x55);
    let unknown_speed_peer = make_ready_propagation_peer(&daemon, 0x56);
    let entry = PropagationEntryRecord {
        transient_id: "d7".repeat(32),
        destination: "1a".repeat(16),
        payload_hex: "26".repeat(32),
        received_at: 1_700_000_620,
        size_bytes: 32,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    for peer in [&fast_peer, &slower_peer, &unknown_speed_peer] {
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
            (unknown_speed_peer.as_str(), 0.0),
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.alive = true;
            record.last_seen = 1_700_000_621;
            record.last_sync_attempt = record.last_seen.saturating_sub(1);
            record.next_sync_attempt = 0;
            record.sync_transfer_rate = rate;
        }
    }

    let selected = daemon
        .select_peer_for_maintenance_sync(1_700_000_621)
        .expect("select maintenance sync peer");

    assert_eq!(selected.as_deref(), Some(unknown_speed_peer.as_str()));
}

#[test]
fn propagation_storage_maintenance_refreshes_peer_snapshot_after_policy_prune_like_python() {
    let daemon = RpcDaemon::test_instance();
    let peer = make_ready_propagation_peer(&daemon, 0x68);
    {
        let mut state = daemon.propagation_state.lock().expect("propagation mutex poisoned");
        state.peer_entry_limit = 16;
        state.peer_entry_limit_per_peer = 1;
    }

    let first = PropagationEntryRecord {
        transient_id: "e1".repeat(32),
        destination: "31".repeat(16),
        payload_hex: "31".repeat(16),
        received_at: 1_700_000_630,
        size_bytes: 16,
        stamp_value: None,
    };
    let second = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "32".repeat(16),
        payload_hex: "32".repeat(16),
        received_at: 1_700_000_631,
        size_bytes: 16,
        stamp_value: None,
    };
    for entry in [&first, &second] {
        daemon.store.upsert_propagation_entry(entry).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark propagation entry unhandled");
    }
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(&peer).expect("peer record");
        record.restored_unhandled_ids = vec![first.transient_id.clone(), second.transient_id.clone()];
    }

    assert_eq!(daemon.maintain_propagation_storage().expect("maintain propagation storage"), 1);

    let persisted_ids = daemon
        .store
        .list_peer_unhandled_propagation_ids(peer.as_str())
        .expect("persisted unhandled ids");
    let snapshot_ids = daemon
        .peers
        .lock()
        .expect("peers mutex poisoned")
        .get(&peer)
        .expect("peer record")
        .restored_unhandled_ids
        .clone();
    assert_eq!(snapshot_ids, persisted_ids);
}
