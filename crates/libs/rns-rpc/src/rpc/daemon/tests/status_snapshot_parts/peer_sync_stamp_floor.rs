#[test]
fn peer_sync_stamp_floor_filters_before_transfer_and_peering_key_gates() {
    // LXMF 1.2.0 LXMPeer.sync uses max(0, cost - flexibility), before offer preparation.
    for (cost, flexibility, values, expected, peering_cost, transfer_limit, sync_limit) in [
        (8, 2, vec![5, 6, 7], vec![6, 7], Some(1), None, 1000),
        (2, 8, vec![0, 1], vec![0, 1], Some(1), None, 1000),
        (8, 2, vec![5], vec![], Some(1), None, 1000),
        (8, 2, vec![5], vec![], None, None, 1000),
        (8, 2, vec![5], vec![], Some(1), Some(16), 1000),
        (8, 2, vec![5], vec![], Some(1), None, 24),
    ] {
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode([2u8; 16]),
        );
        let peer_id = hex::encode([5u8; 16]);
        daemon
            .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
            .expect("initial sync");
        {
            let mut peers = daemon.peers.lock().expect("peers");
            let peer = peers.get_mut(&peer_id).expect("peer");
            peer.propagation_stamp_cost = Some(cost);
            peer.propagation_stamp_cost_flexibility = Some(flexibility);
            peer.peering_cost = peering_cost;
            peer.propagation_transfer_limit = transfer_limit;
            peer.propagation_sync_limit = Some(sync_limit);
        }
        let mut rejected_ids = Vec::new();
        let mut accepted_ids = Vec::new();
        for value in &values {
            let entry = PropagationEntryRecord {
                transient_id: hex::encode([*value as u8; 32]),
                destination: "18".repeat(16),
                payload_hex: "23".repeat(24),
                received_at: 1_700_000_617,
                size_bytes: 24,
                stamp_value: Some(*value),
            };
            daemon.store.upsert_propagation_entry(&entry).expect("store entry");
            daemon
                .store
                .mark_peer_unhandled_propagation(&peer_id, &entry.transient_id)
                .expect("pending");
            if expected.contains(value) {
                accepted_ids.push(entry.transient_id);
            } else {
                rejected_ids.push(entry.transient_id);
            }
        }
        let result = daemon
            .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
            .expect("sync")
            .result
            .expect("result");
        assert_eq!(result["synced"], json!(true), "{result}");
        let propagation = &result["propagation"];
        assert_eq!(propagation["handled"], json!(accepted_ids.len()), "{result}");
        assert_eq!(propagation["transferred"], json!(accepted_ids.len()), "{result}");
        assert_eq!(propagation["rejected"], json!(rejected_ids.len()), "{result}");
        assert_eq!(propagation["rejected_bytes"], json!(24 * rejected_ids.len()));
        assert_eq!(propagation["rejected_ids"], json!(rejected_ids));
        assert_eq!(propagation["handled_ids"], json!(accepted_ids));
        assert_eq!(propagation["transfer_limited"], json!(0));
        assert!(daemon
            .store
            .list_peer_unhandled_propagation(&peer_id)
            .expect("pending")
            .is_empty());
        assert_eq!(
            daemon.store.list_peer_handled_propagation_ids(&peer_id).expect("handled"),
            accepted_ids
        );
        for id in rejected_ids {
            // A peer's minimum does not delete the shared message or mark it delivered.
            assert!(daemon.store.get_propagation_entry(&id).expect("stored entry").is_some());
        }
    }
}

#[test]
fn peer_sync_result_and_event_report_message_accounting() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    let peer_id = hex::encode([4u8; 16]);
    daemon
        .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
        .expect("initial peer sync");
    daemon.event_queue.lock().expect("event_queue mutex poisoned").clear();
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let peer = peers.get_mut(peer_id.as_str()).expect("peer record");
        peer.propagation_stamp_cost = Some(1);
        peer.propagation_stamp_cost_flexibility = Some(1);
        peer.peering_cost = Some(1);
    }

    let entry = PropagationEntryRecord {
        transient_id: "d4".repeat(32),
        destination: "18".repeat(16),
        payload_hex: "22".repeat(24),
        received_at: 1_700_000_616,
        size_bytes: 24,
        stamp_value: Some(12),
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer_id.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(64, "peer_sync", json!({ "peer": peer_id })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(result["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(result["messages"]["offered_bytes"].as_u64(), Some(24));
    assert_eq!(result["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("result handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        result["messages"]["unhandled_ids"].as_array().expect("result unhandled ids"),
        &[] as &[JsonValue]
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
    assert_eq!(event.payload["messages"]["offered"].as_u64(), Some(1));
    assert_eq!(event.payload["messages"]["unhandled"].as_u64(), Some(0));
    assert_eq!(event.payload["messages"]["offered_bytes"].as_u64(), Some(24));
    assert_eq!(event.payload["messages"]["unhandled_bytes"].as_u64(), Some(0));
    assert!(event.payload["messages"].get("handled_ids").is_none());
    assert!(event.payload["messages"].get("unhandled_ids").is_none());
}

#[test]
fn peer_sync_validates_explicit_selection_against_stamp_filtered_offer_without_partial_removal() {
    for wanted_value in [5u8, 6] {
        let daemon = RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            hex::encode([2u8; 16]),
        );
        let peer_id = hex::encode([5u8; 16]);
        daemon
            .handle_rpc(rpc_request(63, "peer_sync", json!({ "peer": peer_id })))
            .expect("initial sync");
        {
            let mut peers = daemon.peers.lock().expect("peers");
            let peer = peers.get_mut(&peer_id).expect("peer");
            peer.propagation_stamp_cost = Some(8);
            peer.propagation_stamp_cost_flexibility = Some(2);
            peer.peering_cost = Some(1);
            peer.propagation_sync_limit = Some(65); // One 24-byte entry + 16 overhead + 24 base.
        }
        for value in [5u8, 6] {
            let entry = PropagationEntryRecord {
                transient_id: hex::encode([value; 32]),
                destination: "18".repeat(16),
                payload_hex: "23".repeat(24),
                received_at: 1_700_000_617,
                size_bytes: 24,
                stamp_value: Some(u32::from(value)),
            };
            daemon.store.upsert_propagation_entry(&entry).expect("store");
            daemon
                .store
                .mark_peer_unhandled_propagation(&peer_id, &entry.transient_id)
                .expect("pending");
        }
        let response = daemon.handle_rpc(rpc_request(
            64,
            "peer_sync",
            json!({
                "peer": peer_id, "wanted_ids": [hex::encode([wanted_value; 32])]
            }),
        ));
        if wanted_value == 5 {
            assert_eq!(
                response.expect_err("below-floor ID is not in the offer").kind(),
                std::io::ErrorKind::InvalidInput
            );
            assert_eq!(
                daemon.store.list_peer_unhandled_propagation(&peer_id).expect("pending").len(),
                2,
                "invalid selection must not remove queue entries"
            );
            assert!(daemon
                .store
                .list_peer_handled_propagation_ids(&peer_id)
                .expect("handled")
                .is_empty());
        } else {
            let result = response.expect("valid selection").result.expect("result");
            assert_eq!(result["propagation"]["transferred_ids"], json!([hex::encode([6u8; 32])]));
            assert_eq!(result["propagation"]["rejected_ids"], json!([hex::encode([5u8; 32])]));
        }
    }
}
