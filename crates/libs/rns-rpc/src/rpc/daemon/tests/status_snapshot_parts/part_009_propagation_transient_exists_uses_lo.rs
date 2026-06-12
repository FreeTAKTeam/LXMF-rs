#[test]
fn propagation_transient_exists_uses_local_propagation_store() {
    let daemon = RpcDaemon::test_instance();
    let transient_id = "ab".repeat(32);
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: "cd".repeat(16),
            payload_hex: "ef".repeat(24),
            received_at: 1_700_000_400,
            size_bytes: 24,
            stamp_value: None,
        })
        .expect("store propagation entry");

    assert!(daemon.propagation_transient_exists(transient_id.as_str()).expect("known transient"));
    assert!(
        !daemon
            .propagation_transient_exists("12".repeat(32).as_str())
            .expect("unknown transient")
    );
}

#[test]
fn propagation_transient_exists_normalizes_case() {
    let daemon = RpcDaemon::test_instance();
    let transient_id = "ac".repeat(32);
    daemon
        .store
        .upsert_propagation_entry(&PropagationEntryRecord {
            transient_id: transient_id.clone(),
            destination: "cd".repeat(16),
            payload_hex: "ef".repeat(24),
            received_at: 1_700_000_401,
            size_bytes: 24,
            stamp_value: None,
        })
        .expect("store propagation entry");

    assert!(
        daemon
            .propagation_transient_exists(transient_id.to_ascii_uppercase().as_str())
            .expect("uppercase transient")
    );
}

#[test]
fn propagation_peer_maintenance_culls_unreachable_non_static_peers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            41,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "static_peers": ["peer-static-unreachable-retained"],
            }),
        ))
        .expect("enable propagation");

    let stale_entry = PropagationEntryRecord {
        transient_id: "c9".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_500,
        size_bytes: 24,
        stamp_value: None,
    };
    let static_entry = PropagationEntryRecord {
        transient_id: "ca".repeat(32),
        destination: "13".repeat(16),
        payload_hex: "13".repeat(24),
        received_at: 1_700_000_501,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&stale_entry).expect("store stale entry");
    daemon.store.upsert_propagation_entry(&static_entry).expect("store static entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-auto-unreachable-cull",
            stale_entry.transient_id.as_str(),
        )
        .expect("mark stale unhandled");
    daemon
        .store
        .mark_peer_unhandled_propagation(
            "peer-static-unreachable-retained",
            static_entry.transient_id.as_str(),
        )
        .expect("mark static unhandled");
    daemon
        .accept_announce_with_metadata(
            "peer-auto-unreachable-cull".to_string(),
            1_700_000_510,
            Some("Cull Candidate".to_string()),
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

    let stale_last_seen = now_i64() - (14 * 24 * 60 * 60) - 1;
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let stale = peers.get_mut("peer-auto-unreachable-cull").expect("stale peer");
        stale.last_seen = stale_last_seen;
        stale.alive = false;
        let static_peer =
            peers.get_mut("peer-static-unreachable-retained").expect("static peer");
        static_peer.last_seen = stale_last_seen;
        static_peer.alive = false;
        static_peer.next_sync_attempt = i64::MAX;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(42, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(1));
    assert_eq!(
        result["culled_peers"].as_array().expect("culled peers"),
        &[json!("peer-auto-unreachable-cull")]
    );
    assert_eq!(result["synced_peer"].as_str(), None);
    let peers = daemon
        .handle_rpc(RpcRequest { id: 43, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-auto-unreachable-cull")));
    assert!(
        rows.iter()
            .any(|row| row["peer"].as_str() == Some("peer-static-unreachable-retained"))
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-unreachable-cull")
            .expect("stale unhandled marks")
            .is_empty()
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-static-unreachable-retained")
            .expect("static unhandled marks"),
        vec![static_entry]
    );
    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-auto-unreachable-cull"));
    assert_eq!(event.payload["reason"].as_str(), Some("max_unreachable"));
}

#[test]
fn propagation_peer_maintenance_cull_replays_restored_queue_before_cleanup_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            43,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");

    daemon
        .accept_announce_with_metadata(
            "peer-auto-unreachable-restored-cull".to_string(),
            1_700_000_511,
            Some("Restored Cull Candidate".to_string()),
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
    let entry = PropagationEntryRecord {
        transient_id: "cb".repeat(32),
        destination: "14".repeat(16),
        payload_hex: "14".repeat(24),
        received_at: 1_700_000_502,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store restored entry");

    let stale_last_seen = now_i64() - (14 * 24 * 60 * 60) - 1;
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let stale = peers
            .get_mut("peer-auto-unreachable-restored-cull")
            .expect("stale peer");
        stale.last_seen = stale_last_seen;
        stale.alive = false;
        stale.restored_unhandled_ids.push(entry.transient_id.clone());
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(44, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(1));
    assert_eq!(
        result["culled_peers"].as_array().expect("culled peers"),
        &[json!("peer-auto-unreachable-restored-cull")]
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("peer unpeer event");
    assert_eq!(
        event.payload["peer"].as_str(),
        Some("peer-auto-unreachable-restored-cull")
    );
    assert_eq!(event.payload["reason"].as_str(), Some("max_unreachable"));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
    assert_eq!(
        event.payload["messages"]["unhandled_ids"]
            .as_array()
            .expect("event unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-auto-unreachable-restored-cull")
            .expect("remaining unhandled")
            .is_empty()
    );
}

#[test]
fn propagation_peer_maintenance_rotates_low_acceptance_autopeers_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            44,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
                "max_peers": 3,
            }),
        ))
        .expect("enable propagation");

    for (peer, timestamp) in [
        ("peer-rotation-low", 1_700_000_610),
        ("peer-rotation-keep-a", 1_700_000_611),
        ("peer-rotation-keep-b", 1_700_000_612),
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

    let recent = now_i64();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        for peer in [
            "peer-rotation-low",
            "peer-rotation-keep-a",
            "peer-rotation-keep-b",
        ] {
            let record = peers.get_mut(peer).expect("peer record");
            record.last_seen = recent;
            record.alive = true;
            record.last_sync_attempt = recent - 1;
            record.offered = 10;
        }
        peers.get_mut("peer-rotation-low").expect("low-rate peer").outgoing = 0;
        peers.get_mut("peer-rotation-keep-a").expect("kept peer").outgoing = 9;
        peers.get_mut("peer-rotation-keep-b").expect("kept peer").outgoing = 10;
    }
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(45, "propagation_peer_maintenance", json!({})))
        .expect("peer maintenance")
        .result
        .expect("peer maintenance result");

    assert_eq!(result["culled"].as_u64(), Some(0));
    assert_eq!(result["rotated"].as_u64(), Some(1));
    assert_eq!(
        result["rotated_peers"].as_array().expect("rotated peers"),
        &[json!("peer-rotation-low")]
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 46, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let rows = peers["peers"].as_array().expect("peer rows");
    assert!(rows.iter().all(|row| row["peer"].as_str() != Some("peer-rotation-low")));
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-keep-a")));
    assert!(rows.iter().any(|row| row["peer"].as_str() == Some("peer-rotation-keep-b")));

    let event = std::iter::from_fn(|| daemon.take_event())
        .find(|event| event.event_type == "peer_unpeer")
        .expect("rotation unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-rotation-low"));
    assert_eq!(event.payload["reason"].as_str(), Some("peer_rotation"));
}
