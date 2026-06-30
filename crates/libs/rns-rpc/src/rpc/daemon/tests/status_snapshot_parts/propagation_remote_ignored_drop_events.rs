#[test]
fn remote_propagation_import_rejects_emit_inbound_dropped_events() {
    for (method, operation, params, expected_peer) in [
        (
            "propagation_remote_sync",
            "propagation_remote_sync",
            json!({ "remote": "remote-node", "peer": "peer-sync-ignored" }),
            Some("peer-sync-ignored"),
        ),
        (
            "propagation_remote_download",
            "propagation_remote_download",
            json!({ "remote": "remote-node" }),
            None,
        ),
        (
            "propagation_remote_fetch",
            "propagation_remote_fetch",
            json!({ "remote": "remote-node" }),
            None,
        ),
    ] {
        assert_remote_ignored_drop_event(method, operation, params, expected_peer);
    }
}

fn assert_remote_ignored_drop_event(
    method: &str,
    operation: &str,
    params: JsonValue,
    expected_peer: Option<&str>,
) {
    let destination = [0x4d_u8; 16];
    let destination_hex = hex::encode(destination);
    let mut payload = destination.to_vec();
    payload.extend_from_slice(b" remote ignored propagation payload");
    let payload_hex = hex::encode(&payload);
    let transient_id = hex::encode(Sha256::digest(&payload));
    let daemon = RpcDaemon::test_instance();

    daemon
        .handle_rpc(rpc_request(
            910,
            "set_delivery_policy",
            json!({ "ignored_destinations": [destination_hex] }),
        ))
        .expect("configure ignored destination");
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({
            "available_count": 1,
            "fetched_count": 1,
            "messages": [{
                "transient_id": transient_id,
                "destination": destination_hex,
                "payload_hex": payload_hex,
            }],
        })),
    }));

    let err = daemon
        .handle_rpc(rpc_request(911, method, params))
        .expect_err("remote ignored destination import should be rejected");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("ignored propagation destination"));

    assert!(
        daemon
            .store
            .get_propagation_entry(transient_id.as_str())
            .expect("lookup ignored remote payload")
            .is_none(),
        "ignored remote payload must not be stored"
    );

    let event = daemon.take_event().expect("remote ignored drop event");
    assert_eq!(event.event_type, "inbound_dropped");
    assert_eq!(event.payload["reason"], json!("delivery_policy_rejected"));
    assert_eq!(event.payload["delivery_kind"], json!("propagation"));
    assert_remote_redacted_identifier(&event.payload["raw_destination_hash"], destination_hex.as_str());
    assert_remote_redacted_identifier(
        &event.payload["resolved_destination_hash"],
        destination_hex.as_str(),
    );
    assert_eq!(event.payload["payload_mode"], json!("full_wire"));
    assert_eq!(event.payload["bytes_len"], json!(payload.len()));
    assert_eq!(event.payload["operation"], json!(operation));
    assert_eq!(event.payload["transient_id"], json!(transient_id));
    if let Some(expected_peer) = expected_peer {
        assert_eq!(event.payload["peer"], json!(expected_peer));
    } else {
        assert!(event.payload.get("peer").is_none(), "remote-only imports should not invent a peer");
    }
}

fn assert_remote_redacted_identifier(value: &JsonValue, raw: &str) {
    let value = value.as_str().expect("redacted identifier");
    assert!(value.starts_with("sha256:"), "identifier must use default hash redaction");
    assert_ne!(value, raw, "identifier must not expose the raw value");
}
