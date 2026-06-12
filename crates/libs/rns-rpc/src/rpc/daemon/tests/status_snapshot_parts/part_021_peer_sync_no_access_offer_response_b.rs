#[test]
fn peer_sync_no_access_offer_response_breaks_peering_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-local-denied" })))
        .expect("initial peer sync");
    let pending = PropagationEntryRecord {
        transient_id: "ac".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-local-denied", pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-local-denied",
                "wanted_ids": 0xf1,
            }),
        ))
        .expect("no-access offer response should break peering")
        .result
        .expect("peer sync result");

    assert_eq!(result["peer"].as_str(), Some("peer-local-denied"));
    assert_eq!(result["offer_response"].as_u64(), Some(0xf1));
    assert_eq!(result["reason"].as_str(), Some("access_denied"));
    assert_eq!(result["unpeered"].as_bool(), Some(true));
    assert_eq!(result["removed"].as_bool(), Some(true));
    assert_eq!(result["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(result["propagation_cleared_bytes"].as_u64(), Some(24));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    assert!(
        peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .all(|row| row["peer"].as_str() != Some("peer-local-denied")),
        "ERROR_NO_ACCESS should remove the local peer record"
    );
    assert!(daemon
        .store
        .list_peer_unhandled_propagation("peer-local-denied")
        .expect("pending propagation")
        .is_empty());
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-local-denied")
            .expect("handled ids")
            .is_empty(),
        "ERROR_NO_ACCESS should clear queue marks without accepting messages"
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_unpeer")
        .cloned()
        .expect("denied access unpeer event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-local-denied"));
    assert_eq!(event.payload["reason"].as_str(), Some("access_denied"));
    assert_eq!(event.payload["offer_response"].as_u64(), Some(0xf1));
    assert_eq!(event.payload["removed"].as_bool(), Some(true));
    assert_eq!(event.payload["propagation_cleared"].as_u64(), Some(1));
    assert_eq!(event.payload["propagation_cleared_bytes"].as_u64(), Some(24));
}

#[test]
fn peer_sync_throttled_offer_response_preserves_peer_queue_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-local-throttled" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-local-throttled").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.75;
    }
    let pending = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_608,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-local-throttled", pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-local-throttled",
                "wanted_ids": 0xf6,
            }),
        ))
        .expect("throttled offer response should postpone local peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["peer"].as_str(), Some("peer-local-throttled"));
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    let last_sync_attempt = result["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 180));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.75));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(true));
    assert_eq!(result["propagation"]["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-local-throttled"))
        .expect("peer row");
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(last_sync_attempt + 180));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-local-throttled")
            .expect("pending propagation"),
        vec![pending.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-local-throttled")
            .expect("handled ids")
            .is_empty(),
        "throttling should preserve queued offers without accepting messages"
    );

    let event = daemon
        .event_queue
        .lock()
        .expect("event_queue mutex poisoned")
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("throttled peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-local-throttled"));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["postpone_reason"].as_str(), Some("throttled"));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["last_sync_attempt"].as_i64(), Some(last_sync_attempt));
    assert_eq!(
        event.payload["next_sync_attempt"].as_i64(),
        Some(last_sync_attempt + 180)
    );
}

#[test]
fn peer_sync_no_identity_offer_response_preserves_peer_for_immediate_retry_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": "peer-local-needs-id" })))
        .expect("initial peer sync");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut("peer-local-needs-id").expect("peer record");
        peer.alive = true;
        peer.sync_backoff = 0;
        peer.next_sync_attempt = 0;
        peer.acceptance_rate = 0.8;
    }
    let pending = PropagationEntryRecord {
        transient_id: "ae".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "12".repeat(24),
        received_at: 1_700_000_609,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation("peer-local-needs-id", pending.transient_id.as_str())
        .expect("mark unhandled");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

    let result = daemon
        .handle_rpc(rpc_request(
            55,
            "peer_sync",
            json!({
                "peer": "peer-local-needs-id",
                "wanted_ids": 0xf0,
            }),
        ))
        .expect("identity-required response should preserve peer for retry")
        .result
        .expect("peer sync result");

    assert_eq!(result["peer"].as_str(), Some("peer-local-needs-id"));
    assert_eq!(result["synced"].as_bool(), Some(false));
    assert_eq!(result["reason"].as_str(), Some("identity_required"));
    assert_eq!(result["offer_response"].as_u64(), Some(0xf0));
    assert_eq!(result["alive"].as_bool(), Some(true));
    assert_eq!(result["sync_backoff"].as_u64(), Some(0));
    assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
    assert_eq!(result["acceptance_rate"].as_f64(), Some(0.8));
    assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-local-needs-id")
            .expect("pending propagation"),
        vec![pending.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_handled_propagation_ids("peer-local-needs-id")
            .expect("handled ids")
            .is_empty(),
        "identity-required response should not accept offered messages"
    );

    let peers = daemon
        .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-local-needs-id"))
        .expect("peer row");
    let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
    assert!(last_sync_attempt > 0);
    assert_eq!(row["alive"].as_bool(), Some(true));
    assert_eq!(row["sync_backoff"].as_u64(), Some(0));
    assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));

    let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
    assert!(
        events.iter().all(|event| event.event_type != "peer_unpeer"),
        "identity-required response should not break peering"
    );
    let event = events
        .iter()
        .rev()
        .find(|event| event.event_type == "peer_sync")
        .cloned()
        .expect("identity-required peer sync event");
    assert_eq!(event.payload["peer"].as_str(), Some("peer-local-needs-id"));
    assert_eq!(event.payload["synced"].as_bool(), Some(false));
    assert_eq!(event.payload["reason"].as_str(), Some("identity_required"));
    assert_eq!(event.payload["offer_response"].as_u64(), Some(0xf0));
    assert_eq!(event.payload["alive"].as_bool(), Some(true));
    assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
    assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
}

#[test]
fn peer_sync_retryable_offer_responses_preserve_peer_queue_like_python() {
    for (suffix, offer_response, reason) in [
        ("invalid-key", 0xf3, "invalid_key"),
        ("invalid-data", 0xf4, "invalid_data"),
        ("invalid-stamp", 0xf5, "invalid_stamp"),
        ("unknown", 0xf2, "peer_offer_error"),
        ("not-found", 0xfd, "not_found"),
        ("timeout", 0xfe, "timeout"),
    ] {
        let daemon = RpcDaemon::test_instance();
        let peer_id = format!("peer-local-{suffix}");
        daemon
            .handle_rpc(rpc_request(52, "peer_sync", json!({ "peer": peer_id })))
            .expect("initial peer sync");
        {
            let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
            let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
            peer.alive = true;
            peer.sync_backoff = 0;
            peer.next_sync_attempt = 0;
            peer.acceptance_rate = 0.6;
        }
        let pending = PropagationEntryRecord {
            transient_id: "af".repeat(32),
            destination: "12".repeat(16),
            payload_hex: "12".repeat(24),
            received_at: 1_700_000_610,
            size_bytes: 24,
            stamp_value: None,
        };
        daemon.store.upsert_propagation_entry(&pending).expect("store propagation entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer_id.as_str(), pending.transient_id.as_str())
            .expect("mark unhandled");
        daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();

        let result = daemon
            .handle_rpc(rpc_request(
                55,
                "peer_sync",
                json!({
                    "peer": peer_id,
                    "wanted_ids": offer_response,
                }),
            ))
            .expect("retryable response should preserve peer queue for retry")
            .result
            .expect("peer sync result");

        assert_eq!(result["peer"].as_str(), Some(peer_id.as_str()));
        assert_eq!(result["synced"].as_bool(), Some(false));
        assert_eq!(result["state"].as_u64(), Some(0xfe));
        assert_eq!(result["state_name"].as_str(), Some("failed"));
        assert_eq!(result["reason"].as_str(), Some(reason));
        assert_eq!(result["offer_response"].as_u64(), Some(offer_response));
        assert_eq!(result["alive"].as_bool(), Some(true));
        assert_eq!(result["sync_backoff"].as_u64(), Some(0));
        assert_eq!(result["next_sync_attempt"].as_i64(), Some(0));
        assert_eq!(result["acceptance_rate"].as_f64(), Some(0.6));
        assert_eq!(result["propagation"]["handled"].as_u64(), Some(0));
        assert_eq!(result["propagation"]["postponed"].as_bool(), Some(false));
        assert_eq!(result["propagation"]["state"].as_u64(), Some(0xfe));
        assert_eq!(result["propagation"]["state_name"].as_str(), Some("failed"));
        assert_eq!(
            daemon
                .store
                .list_peer_unhandled_propagation(peer_id.as_str())
                .expect("pending propagation"),
            vec![pending.clone()]
        );
        assert!(
            daemon
                .store
                .list_peer_handled_propagation_ids(peer_id.as_str())
                .expect("handled ids")
                .is_empty(),
            "retryable response should not accept offered messages"
        );

        let peers = daemon
            .handle_rpc(RpcRequest { id: 56, method: "list_peers".to_string(), params: None })
            .expect("list peers")
            .result
            .expect("list peers result");
        let row = peers["peers"]
            .as_array()
            .expect("peer rows")
            .iter()
            .find(|row| row["peer"].as_str() == Some(peer_id.as_str()))
            .expect("peer row");
        let last_sync_attempt = row["last_sync_attempt"].as_i64().expect("last sync attempt");
        assert!(last_sync_attempt > 0);
        assert_eq!(row["alive"].as_bool(), Some(true));
        assert_eq!(row["sync_backoff"].as_u64(), Some(0));
        assert_eq!(row["next_sync_attempt"].as_i64(), Some(0));

        let events = daemon.event_queue.lock().expect("event_queue mutex poisoned");
        assert!(
            events.iter().all(|event| event.event_type != "peer_unpeer"),
            "retryable response should not break peering"
        );
        let event = events
            .iter()
            .rev()
            .find(|event| event.event_type == "peer_sync")
            .cloned()
            .expect("retryable peer sync event");
        assert_eq!(event.payload["peer"].as_str(), Some(peer_id.as_str()));
        assert_eq!(event.payload["synced"].as_bool(), Some(false));
        assert_eq!(event.payload["state"].as_u64(), Some(0xfe));
        assert_eq!(event.payload["state_name"].as_str(), Some("failed"));
        assert_eq!(event.payload["reason"].as_str(), Some(reason));
        assert_eq!(event.payload["offer_response"].as_u64(), Some(offer_response));
        assert_eq!(event.payload["alive"].as_bool(), Some(true));
        assert_eq!(event.payload["sync_backoff"].as_u64(), Some(0));
        assert_eq!(event.payload["next_sync_attempt"].as_i64(), Some(0));
        assert_eq!(event.payload["propagation"]["state"].as_u64(), Some(0xfe));
        assert_eq!(event.payload["propagation"]["state_name"].as_str(), Some("failed"));
    }
}
