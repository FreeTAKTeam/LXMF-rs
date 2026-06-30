#[tokio::test]
async fn local_propagated_delivery_marks_processed_transient_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 55,
            method: "propagation_enable".to_string(),
            params: Some(serde_json::json!({
                "enabled": true,
                "target_cost": 1,
            })),
        })
        .expect("enable propagation");
    let delivery_private = PrivateIdentity::new_from_rand(OsRng);
    let source_private = PrivateIdentity::new_from_rand(OsRng);
    let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
        delivery_private.clone(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let source_destination = SingleInputDestination::new(
        source_private.clone(),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut destination_hash = [0u8; 16];
    {
        let destination = delivery_destination.lock().await;
        destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
    }
    daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
    let mut source_hash = [0u8; 16];
    source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

    let wire = build_wire_message_with_options(
        source_hash,
        destination_hash,
        "processed propagated title",
        "processed propagated content",
        None,
        &to_core_private_identity(&source_private),
        None,
        None,
        None,
    )
    .expect("wire");
    let (envelope, transient_id, stamped_transient) = {
        let destination = delivery_destination.lock().await;
        let message = WireMessage::unpack(&wire).expect("wire unpack");
        let (transient, transient_id) = message
            .pack_propagation_transient_with_rng(
                &to_core_identity(destination.identity.as_identity()),
                OsRng,
            )
            .expect("propagation transient");
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
        let envelope = WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
            .expect("propagation envelope");
        let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
            rmp_serde::from_slice(&envelope).expect("unpack propagation envelope");
        (
            envelope,
            hex::encode(transient_id),
            messages.into_iter().next().expect("stamped transient"),
        )
    };

    let ingested = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
        .await
        .expect("ingest propagation envelope");
    assert_eq!(ingested, 1);
    assert!(
        daemon
            .local_propagation_processed_mark_exists(transient_id.as_str())
            .expect("processed mark lookup"),
        "local propagated delivery should mark the transient processed"
    );
    while daemon.take_event().is_some() {}

    let local_replay =
        ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
            .await
            .expect("replay local propagation envelope");
    assert_eq!(local_replay, 1);
    let duplicate_event = daemon.take_event().expect("duplicate local propagation event");
    assert_eq!(duplicate_event.event_type, "inbound_dropped");
    assert_eq!(duplicate_event.payload["reason"], serde_json::json!("duplicate"));
    assert_eq!(duplicate_event.payload["delivery_kind"], serde_json::json!("propagation"));
    assert_eq!(duplicate_event.payload["payload_mode"], serde_json::json!("full_wire"));
    assert_eq!(duplicate_event.payload["bytes_len"], serde_json::json!(stamped_transient.len()));
    assert_eq!(duplicate_event.payload["transient_id"], serde_json::json!(transient_id));
    assert_eq!(
        duplicate_event.payload["detail"],
        serde_json::json!("transient already processed locally")
    );
    let raw_destination = hex::encode(destination_hash);
    assert!(
        duplicate_event.payload["raw_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination)
    );
    assert!(
        duplicate_event.payload["resolved_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination)
    );
    assert!(
        daemon.take_event().is_none(),
        "duplicate local propagation replay should emit one event"
    );

    let (
        duplicate_record_envelope,
        duplicate_record_transient_id,
        duplicate_record_stamped_transient,
    ) = {
        let destination = delivery_destination.lock().await;
        let message = WireMessage::unpack(&wire).expect("wire unpack");
        let (transient, transient_id) = message
            .pack_propagation_transient_with_rng(
                &to_core_identity(destination.identity.as_identity()),
                OsRng,
            )
            .expect("propagation transient");
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
        let envelope = WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
            .expect("propagation envelope");
        let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
            rmp_serde::from_slice(&envelope).expect("unpack propagation envelope");
        (
            envelope,
            hex::encode(transient_id),
            messages.into_iter().next().expect("stamped transient"),
        )
    };
    assert_ne!(
        duplicate_record_transient_id, transient_id,
        "second local delivery replay must exercise message-id duplicate handling"
    );

    let duplicate_record =
        ingest_propagation_envelope(&daemon, &duplicate_record_envelope, Some(&delivery_destination))
            .await
            .expect("duplicate-message local propagation envelope");
    assert_eq!(duplicate_record, 1);
    assert!(
        daemon
            .local_propagation_processed_mark_exists(duplicate_record_transient_id.as_str())
            .expect("duplicate-message processed mark lookup"),
        "message-id duplicate should still mark the new transient processed"
    );
    let duplicate_record_event =
        daemon.take_event().expect("duplicate-message local propagation event");
    assert_eq!(duplicate_record_event.event_type, "inbound_dropped");
    assert_eq!(duplicate_record_event.payload["reason"], serde_json::json!("duplicate"));
    assert_eq!(
        duplicate_record_event.payload["delivery_kind"],
        serde_json::json!("propagation")
    );
    assert_eq!(
        duplicate_record_event.payload["payload_mode"],
        serde_json::json!("full_wire")
    );
    assert_eq!(
        duplicate_record_event.payload["bytes_len"],
        serde_json::json!(duplicate_record_stamped_transient.len())
    );
    assert_eq!(
        duplicate_record_event.payload["transient_id"],
        serde_json::json!(duplicate_record_transient_id)
    );
    assert_eq!(
        duplicate_record_event.payload["detail"],
        serde_json::json!("message already exists locally")
    );
    assert!(
        duplicate_record_event.payload["raw_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination)
    );
    assert!(
        duplicate_record_event.payload["resolved_destination_hash"]
            .as_str()
            .is_some_and(|value| value.starts_with("sha256:") && value != raw_destination)
    );
    assert!(
        daemon.take_event().is_none(),
        "duplicate-message local propagation should emit one event"
    );

    let replay_status = daemon
        .handle_rpc(RpcRequest {
            id: 56,
            method: "propagation_status".to_string(),
            params: None,
        })
        .expect("replay propagation status")
        .result
        .expect("replay propagation status result");
    assert_eq!(
        replay_status["propagation"]["client_propagation_messages_received"].as_u64(),
        Some(1)
    );
    assert_eq!(replay_status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(replay_status["propagation"]["last_ingest_count"].as_u64(), Some(0));

    let duplicate = daemon
        .handle_rpc(RpcRequest {
            id: 57,
            method: "propagation_ingest".to_string(),
            params: Some(serde_json::json!({
                "transient_id": transient_id.as_str(),
                "payload_hex": hex::encode(stamped_transient),
            })),
        })
        .expect("duplicate propagation ingest")
        .result
        .expect("duplicate propagation ingest result");
    assert_eq!(duplicate["ingested_count"].as_u64(), Some(0));
    assert_eq!(duplicate["duplicate_count"].as_u64(), Some(1));

    let status = daemon
        .handle_rpc(RpcRequest {
            id: 58,
            method: "propagation_status".to_string(),
            params: None,
        })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["last_ingest_count"].as_u64(), Some(0));
}

#[tokio::test]
async fn local_policy_dropped_propagation_marks_processed_transient_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 59,
            method: "propagation_enable".to_string(),
            params: Some(serde_json::json!({
                "enabled": true,
                "target_cost": 1,
            })),
        })
        .expect("enable propagation");
    let delivery_private = PrivateIdentity::new_from_rand(OsRng);
    let source_private = PrivateIdentity::new_from_rand(OsRng);
    let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
        delivery_private.clone(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let source_destination = SingleInputDestination::new(
        source_private.clone(),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut destination_hash = [0u8; 16];
    {
        let destination = delivery_destination.lock().await;
        destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
    }
    daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
    let mut source_hash = [0u8; 16];
    source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
    daemon
        .handle_rpc(RpcRequest {
            id: 60,
            method: "set_delivery_policy".to_string(),
            params: Some(serde_json::json!({
                "ignored_destinations": [hex::encode(source_hash)],
            })),
        })
        .expect("set delivery policy");

    let wire = build_wire_message_with_options(
        source_hash,
        destination_hash,
        "processed dropped propagated title",
        "processed dropped propagated content",
        None,
        &to_core_private_identity(&source_private),
        None,
        None,
        None,
    )
    .expect("wire");
    let (envelope, transient_id, stamped_transient) = {
        let destination = delivery_destination.lock().await;
        let message = WireMessage::unpack(&wire).expect("wire unpack");
        let (transient, transient_id) = message
            .pack_propagation_transient_with_rng(
                &to_core_identity(destination.identity.as_identity()),
                OsRng,
            )
            .expect("propagation transient");
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
        let envelope = WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
            .expect("propagation envelope");
        let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
            rmp_serde::from_slice(&envelope).expect("unpack propagation envelope");
        (
            envelope,
            hex::encode(transient_id),
            messages.into_iter().next().expect("stamped transient"),
        )
    };

    let ingested = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
        .await
        .expect("ingest propagation envelope");
    assert_eq!(ingested, 1);
    let event = daemon.take_event().expect("policy drop event");
    assert_eq!(event.event_type, "inbound_dropped");
    assert_eq!(event.payload["reason"], serde_json::json!("delivery_policy_rejected"));
    assert!(
        daemon
            .local_propagation_processed_mark_exists(transient_id.as_str())
            .expect("processed mark lookup"),
        "policy-handled local propagation should mark the transient processed"
    );

    let duplicate = daemon
        .handle_rpc(RpcRequest {
            id: 61,
            method: "propagation_ingest".to_string(),
            params: Some(serde_json::json!({
                "transient_id": transient_id.as_str(),
                "payload_hex": hex::encode(stamped_transient),
            })),
        })
        .expect("duplicate propagation ingest")
        .result
        .expect("duplicate propagation ingest result");
    assert_eq!(duplicate["ingested_count"].as_u64(), Some(0));
    assert_eq!(duplicate["duplicate_count"].as_u64(), Some(1));
}

#[tokio::test]
async fn peer_replay_of_processed_local_propagation_refreshes_relay_marks_only() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 62,
            method: "propagation_enable".to_string(),
            params: Some(serde_json::json!({
                "enabled": true,
                "target_cost": 1,
            })),
        })
        .expect("enable propagation");
    let delivery_private = PrivateIdentity::new_from_rand(OsRng);
    let source_private = PrivateIdentity::new_from_rand(OsRng);
    let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
        delivery_private.clone(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let source_destination = SingleInputDestination::new(
        source_private.clone(),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut destination_hash = [0u8; 16];
    {
        let destination = delivery_destination.lock().await;
        destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
    }
    daemon.set_delivery_destination_hash(Some(hex::encode(destination_hash)));
    let mut source_hash = [0u8; 16];
    source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

    let wire = build_wire_message_with_options(
        source_hash,
        destination_hash,
        "peer replay propagated title",
        "peer replay propagated content",
        None,
        &to_core_private_identity(&source_private),
        None,
        None,
        None,
    )
    .expect("wire");
    let (envelope, transient_id) = {
        let destination = delivery_destination.lock().await;
        let message = WireMessage::unpack(&wire).expect("wire unpack");
        let (transient, transient_id) = message
            .pack_propagation_transient_with_rng(
                &to_core_identity(destination.identity.as_identity()),
                OsRng,
            )
            .expect("propagation transient");
        let stamp = generate_propagation_stamp(&transient_id, 1).expect("propagation stamp");
        (
            WireMessage::pack_propagation_envelope(1.0, &transient, Some(&stamp))
                .expect("propagation envelope"),
            hex::encode(transient_id),
        )
    };

    let first = ingest_propagation_envelope(&daemon, &envelope, Some(&delivery_destination))
        .await
        .expect("first local propagation ingest");
    assert_eq!(first, 1);

    let propagation_peer = hex::encode([0x82_u8; 16]);
    let relay_peer = hex::encode([0x83_u8; 16]);
    for (id, peer) in [(63, &propagation_peer), (64, &relay_peer)] {
        daemon
            .handle_rpc(RpcRequest {
                id,
                method: "peer_sync".to_string(),
                params: Some(serde_json::json!({ "peer": peer })),
            })
            .expect("seed propagation peer");
    }

    let replay = ingest_propagation_envelope_from_peer(
        &daemon,
        &envelope,
        Some(&delivery_destination),
        Some(&propagation_peer),
    )
    .await
    .expect("peer replay local propagation envelope");
    assert_eq!(replay, 1);

    let source = peer_row(&daemon, propagation_peer.as_str(), 65);
    assert_eq!(source["messages"]["incoming"].as_u64(), Some(1));
    assert_eq!(source["rx_bytes"].as_u64(), Some(0));
    assert!(
        daemon
            .has_peer_completed_propagation_mark(propagation_peer.as_str(), transient_id.as_str())
            .expect("completed propagation mark lookup"),
        "processed local replay from a peer should still mark source peer handled"
    );
    let relay = peer_row(&daemon, relay_peer.as_str(), 66);
    assert_eq!(
        relay["messages"]["unhandled_ids"].as_array().expect("relay unhandled ids"),
        &[serde_json::json!(transient_id.as_str())],
        "processed local replay from a peer should still fan out to relay peers"
    );

    let status = daemon
        .handle_rpc(RpcRequest {
            id: 67,
            method: "propagation_status".to_string(),
            params: None,
        })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(status["propagation"]["client_propagation_messages_received"].as_u64(), Some(1));
    assert_eq!(status["propagation"]["total_ingested"].as_u64(), Some(1));
}
