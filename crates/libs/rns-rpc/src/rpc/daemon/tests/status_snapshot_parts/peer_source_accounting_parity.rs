fn source_accounting_entry(seed: u8, received_at: i64) -> PropagationEntryRecord {
    let payload = vec![seed; 24];
    PropagationEntryRecord {
        transient_id: hex::encode(Sha256::digest(payload.as_slice())),
        destination: hex::encode([seed; 16]),
        payload_hex: hex::encode(payload.as_slice()),
        received_at,
        size_bytes: payload.len() as u64,
        stamp_value: None,
    }
}

#[test]
fn peer_sync_haves_completion_marks_done_and_allows_later_unrelated_work() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xc1);
    let wanted = source_accounting_entry(0xc1, 1_700_000_901);
    let already_have = source_accounting_entry(0xc2, 1_700_000_902);
    for entry in [&wanted, &already_have] {
        daemon.store.upsert_propagation_entry(entry).expect("store offered entry");
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark offered entry unhandled");
    }

    let ack = daemon
        .handle_rpc(rpc_request(
            90,
            "peer_sync",
            json!({
                "peer": peer.as_str(),
                "wanted_ids": [wanted.transient_id.as_str()],
            }),
        ))
        .expect("peer sync with haves acknowledgement")
        .result
        .expect("peer sync result");
    assert_eq!(ack["propagation"]["handled"].as_u64(), Some(2));
    assert_eq!(ack["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(ack["messages"]["offered"].as_u64(), Some(2));
    assert_eq!(ack["messages"]["outgoing"].as_u64(), Some(1));
    let mut expected_completed = vec![wanted.transient_id.clone(), already_have.transient_id.clone()];
    expected_completed.sort();
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("completed ids after haves"),
        expected_completed
    );

    let future = source_accounting_entry(0xc3, 1_700_000_903);
    daemon.store.upsert_propagation_entry(&future).expect("store later unrelated entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), future.transient_id.as_str())
        .expect("mark later unrelated entry unhandled");

    let later = daemon
        .handle_rpc(rpc_request(91, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("later peer sync")
        .result
        .expect("later peer sync result");
    assert_eq!(later["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        later["propagation"]["transferred_ids"].as_array().expect("transferred ids"),
        &[json!(future.transient_id.as_str())]
    );
    assert_eq!(later["messages"]["offered"].as_u64(), Some(3));
    assert_eq!(later["messages"]["outgoing"].as_u64(), Some(2));
}

#[test]
fn retained_payload_not_reoffered_to_completed_peer_but_serves_other_peers() {
    let (daemon, completed_peer) = ready_propagation_peer_daemon(0xd1);
    let relay_peer = make_ready_propagation_peer(&daemon, 0xd2);
    let entry = source_accounting_entry(0xd3, 1_700_000_904);
    daemon.store.upsert_propagation_entry(&entry).expect("store retained entry");
    for peer in [&completed_peer, &relay_peer] {
        daemon
            .store
            .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
            .expect("mark retained entry unhandled");
    }

    let first = daemon
        .handle_rpc(rpc_request(92, "peer_sync", json!({ "peer": completed_peer.as_str() })))
        .expect("completed peer sync")
        .result
        .expect("completed peer sync result");
    assert_eq!(first["propagation"]["transferred"].as_u64(), Some(1));

    let repeated = daemon
        .handle_rpc(rpc_request(93, "peer_sync", json!({ "peer": completed_peer.as_str() })))
        .expect("repeated completed peer sync")
        .result
        .expect("repeated completed peer sync result");
    assert_eq!(repeated["propagation"]["transferred"].as_u64(), Some(0));
    assert!(repeated["propagation"]["messages"].as_array().expect("messages").is_empty());
    assert_eq!(
        repeated["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );

    let fetched = daemon
        .handle_rpc(rpc_request(
            94,
            "propagation_fetch",
            json!({ "transient_id": entry.transient_id.as_str() }),
        ))
        .expect("fetch retained payload")
        .result
        .expect("fetch retained payload result");
    assert_eq!(fetched["payload_hex"].as_str(), Some(entry.payload_hex.as_str()));

    let relay = daemon
        .handle_rpc(rpc_request(95, "peer_sync", json!({ "peer": relay_peer.as_str() })))
        .expect("relay peer sync")
        .result
        .expect("relay peer sync result");
    assert_eq!(relay["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        relay["propagation"]["transferred_ids"].as_array().expect("relay transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
}

#[test]
fn repeated_remote_download_does_not_double_count_source_receive_bytes() {
    let payload = b"duplicate-remote-download-source-accounting";
    let payload_hex = hex::encode(payload);
    let transient_id = hex::encode(Sha256::digest(payload));
    let source_peer = "remote-download-duplicate-source";
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(96, "peer_sync", json!({ "peer": source_peer })))
        .expect("seed source peer");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "downloaded_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    for request_id in [97, 98] {
        daemon
            .handle_rpc(rpc_request(
                request_id,
                "propagation_remote_download",
                json!({ "remote": source_peer }),
            ))
            .expect("remote download from source peer");
    }

    let peers = daemon
        .handle_rpc(RpcRequest { id: 99, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let source_row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some(source_peer))
        .expect("source peer row");
    assert_eq!(source_row["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["incoming"].as_u64(), Some(1));
    assert_eq!(source_row["rx_bytes"].as_u64(), Some(payload.len() as u64));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(source_peer)
            .expect("source handled ids"),
        vec![transient_id]
    );
}

#[test]
fn peer_sync_fetch_without_prior_offer_mark_records_transfer_and_handled_id() {
    let (daemon, peer) = ready_propagation_peer_daemon(0xe1);
    let entry = source_accounting_entry(0xe2, 1_700_000_905);
    daemon.store.upsert_propagation_entry(&entry).expect("store entry without offer mark");
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pre-sync unhandled entries")
            .is_empty(),
        "test setup should start before a peer offer row exists"
    );

    let result = daemon
        .handle_rpc(rpc_request(100, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("peer sync without prior offer mark")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["messages"]["outgoing"].as_u64(), Some(1));
    assert!(result["tx_bytes"].as_u64().is_some_and(|value| value > 0));
    assert_eq!(
        result["messages"]["handled_ids"].as_array().expect("handled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![entry.transient_id]
    );
}
