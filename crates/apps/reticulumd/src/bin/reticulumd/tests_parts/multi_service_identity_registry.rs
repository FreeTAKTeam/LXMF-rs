fn session_rpc(
    daemon: &RpcDaemon,
    session_id: &str,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> rns_rpc::RpcResponse {
    let request = RpcRequest {
        id,
        method: method.to_string(),
        params: Some(params),
    };
    let frame = rns_rpc::rpc::codec::encode_frame(&request).expect("encode request");
    let response = daemon
        .handle_framed_request_for_session(session_id, frame.as_slice())
        .expect("handle framed request");
    rns_rpc::rpc::codec::decode_frame(response.as_slice()).expect("decode response")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn service_identities_are_distinct_persistent_and_session_scoped() {
    use base64::Engine as _;
    use rns_rpc::ServiceIdentityBridge as _;

    let (daemon, bridge) = test_transport_bridge_fixture().await;
    daemon.set_service_identity_bridge(bridge.clone());
    let identity_a = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity_b = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let import = |identity: &PrivateIdentity, display_name: &str| {
        json!({
            "bundle_base64": base64::engine::general_purpose::STANDARD
                .encode(identity.to_private_key_bytes()),
            "display_name": display_name,
            "capabilities": ["lxmf.delivery", "test.service"],
            "metadata": {"test": true}
        })
    };

    let imported_a = session_rpc(
        &daemon,
        "service-a",
        1,
        "sdk_identity_import_v2",
        import(&identity_a, "Service A"),
    );
    assert!(imported_a.error.is_none(), "{:?}", imported_a.error);
    let imported_b = session_rpc(
        &daemon,
        "service-b",
        2,
        "sdk_identity_import_v2",
        import(&identity_b, "Service B"),
    );
    assert!(imported_b.error.is_none(), "{:?}", imported_b.error);
    let bundle_a = &imported_a.result.as_ref().expect("A result")["identity"];
    let bundle_b = &imported_b.result.as_ref().expect("B result")["identity"];
    assert_ne!(bundle_a["identity"], bundle_b["identity"]);
    assert_ne!(
        bundle_a["delivery_destination"],
        bundle_b["delivery_destination"]
    );
    assert_eq!(bundle_a["display_name"], "Service A");
    assert_eq!(bundle_b["display_name"], "Service B");

    let list_a = session_rpc(&daemon, "service-a", 3, "sdk_identity_list_v2", json!({}));
    let list_b = session_rpc(&daemon, "service-b", 4, "sdk_identity_list_v2", json!({}));
    assert_eq!(
        list_a.result.as_ref().expect("list A")["identities"]
            .as_array()
            .expect("A identities")
            .len(),
        1
    );
    assert_eq!(
        list_b.result.as_ref().expect("list B")["identities"]
            .as_array()
            .expect("B identities")
            .len(),
        1
    );

    let identity_b_hash = bundle_b["identity"].as_str().expect("B identity");
    let forbidden = session_rpc(
        &daemon,
        "service-a",
        5,
        "sdk_identity_activate_v2",
        json!({"identity": identity_b_hash}),
    );
    assert_eq!(
        forbidden.error.as_ref().map(|error| error.code.as_str()),
        Some("SDK_SECURITY_IDENTITY_FORBIDDEN")
    );

    let same_key = session_rpc(
        &daemon,
        "service-a-reconnect",
        6,
        "sdk_identity_import_v2",
        import(&identity_a, "Service A Reconnected"),
    );
    assert_eq!(
        same_key.result.as_ref().expect("same-key result")["identity"]["identity"],
        bundle_a["identity"]
    );
    assert_eq!(
        same_key.result.as_ref().expect("same-key result")["identity"]
            ["delivery_destination"],
        bundle_a["delivery_destination"]
    );

    let invalid = session_rpc(
        &daemon,
        "invalid-service",
        7,
        "sdk_identity_import_v2",
        json!({
            "bundle_base64": base64::engine::general_purpose::STANDARD.encode([1_u8, 2, 3])
        }),
    );
    assert_eq!(
        invalid.error.as_ref().map(|error| error.code.as_str()),
        Some("SDK_VALIDATION_INVALID_ARGUMENT")
    );

    let records = bridge.list_service_identities().expect("registry");
    assert!(records.iter().any(|record| {
        record.identity == bundle_a["identity"].as_str().expect("A identity")
            && record.display_name.as_deref() == Some("Service A Reconnected")
    }));
    assert!(records.iter().any(|record| {
        record.identity == identity_b_hash && record.display_name.as_deref() == Some("Service B")
    }));

    let service_identity_dir = bridge.service_identity_storage_dir();
    assert!(service_identity_dir.join("registry.json").is_file());
    drop(daemon);
    drop(bridge);

    let (restarted_daemon, restarted_bridge, _, _) =
        test_transport_bridge_fixture_with_peer_at(service_identity_dir).await;
    let loaded = restarted_bridge
        .load_persisted_service_identities()
        .await
        .expect("reload persisted service identities");
    assert_eq!(loaded, 2);
    restarted_daemon.set_service_identity_bridge(restarted_bridge.clone());
    let reconnected = session_rpc(
        &restarted_daemon,
        "service-a-after-restart",
        8,
        "sdk_identity_import_v2",
        import(&identity_a, "Service A After Restart"),
    );
    assert_eq!(
        reconnected.result.as_ref().expect("restart result")["identity"]
            ["delivery_destination"],
        bundle_a["delivery_destination"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn paper_encoding_uses_selected_service_identity() {
    use base64::Engine as _;

    let (daemon, bridge, recipient, recipient_hex) =
        test_transport_bridge_fixture_with_peer().await;
    daemon.set_service_identity_bridge(bridge);
    let service_identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let imported = session_rpc(
        &daemon,
        "paper-service",
        20,
        "sdk_identity_import_v2",
        json!({
            "bundle_base64": base64::engine::general_purpose::STANDARD
                .encode(service_identity.to_private_key_bytes()),
            "display_name": "Paper Service"
        }),
    );
    let source = imported.result.expect("import result")["identity"]["delivery_destination"]
        .as_str()
        .expect("delivery destination")
        .to_string();

    let send = session_rpc(
        &daemon,
        "paper-service",
        21,
        "send_message_v2",
        json!({
            "id": "service-paper-message",
            "source": source,
            "destination": recipient_hex,
            "title": "Service paper",
            "content": "selected identity",
            "method": "paper"
        }),
    );
    assert!(send.error.is_none(), "{:?}", send.error);

    let encoded = session_rpc(
        &daemon,
        "paper-service",
        22,
        "sdk_paper_encode_v2",
        json!({ "message_id": "service-paper-message" }),
    );
    assert!(encoded.error.is_none(), "{:?}", encoded.error);
    let uri = encoded.result.expect("encode result")["envelope"]["uri"]
        .as_str()
        .expect("paper uri")
        .to_string();
    let wire = WireMessage::unpack_paper_uri(uri.as_str(), &recipient).expect("decode paper uri");

    assert_eq!(hex::encode(wire.source), source);
}
