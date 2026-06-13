#[test]
fn sdk_configure_v2_rejects_out_of_bounds_event_stream_limits() {
    let daemon = RpcDaemon::test_instance();
    let below_min_batch = daemon
        .handle_rpc(rpc_request(
            434,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_batch_bytes": 512 } }
            }),
        ))
        .expect("configure");
    assert_eq!(below_min_batch.error.expect("error").code, "SDK_VALIDATION_INVALID_ARGUMENT");

    let extension_limit_overflow = daemon
        .handle_rpc(rpc_request(
            435,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "max_extension_keys": 64 } }
            }),
        ))
        .expect("configure");
    assert_eq!(
        extension_limit_overflow.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT"
    );

    let unknown_event_stream_key = daemon
        .handle_rpc(rpc_request(
            4351,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": { "event_stream": { "unknown_limit": 10 } }
            }),
        ))
        .expect("configure");
    assert_eq!(unknown_event_stream_key.error.expect("error").code, "SDK_CONFIG_UNKNOWN_KEY");

    let inconsistent_event_and_batch = daemon
        .handle_rpc(rpc_request(
            436,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_stream": {
                        "max_event_bytes": 4096,
                        "max_batch_bytes": 2048
                    }
                }
            }),
        ))
        .expect("configure");
    assert_eq!(
        inconsistent_event_and_batch.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT"
    );
}

#[test]
fn sdk_configure_v2_validates_and_applies_store_forward_policy_patch() {
    let daemon = RpcDaemon::test_instance();

    let invalid = daemon
        .handle_rpc(rpc_request(
            4361,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 0
                    }
                }
            }),
        ))
        .expect("configure invalid");
    assert_eq!(
        invalid.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "store_forward max_messages=0 should fail validation"
    );

    let valid = daemon
        .handle_rpc(rpc_request(
            4362,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "store_forward": {
                        "max_messages": 1024,
                        "max_message_age_ms": 120000,
                        "capacity_policy": "drop_oldest",
                        "eviction_priority": "terminal_first"
                    }
                }
            }),
        ))
        .expect("configure valid");
    assert!(valid.error.is_none());
    assert_eq!(valid.result.expect("result")["revision"], json!(1));

    let runtime_config =
        daemon.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
    assert_eq!(runtime_config["store_forward"]["max_messages"], json!(1024));
    assert_eq!(runtime_config["store_forward"]["capacity_policy"], json!("drop_oldest"));
}

#[test]
fn sdk_configure_v2_validates_and_applies_event_sink_patch() {
    let daemon = RpcDaemon::test_instance();

    let invalid = daemon
        .handle_rpc(rpc_request(
            4363,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "allow_kinds": []
                    }
                }
            }),
        ))
        .expect("configure invalid");
    assert_eq!(
        invalid.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "event_sink allow_kinds=[] should fail validation"
    );

    let valid = daemon
        .handle_rpc(rpc_request(
            4364,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "event_sink": {
                        "enabled": true,
                        "max_event_bytes": 32768,
                        "allow_kinds": ["webhook", "mqtt"]
                    }
                }
            }),
        ))
        .expect("configure valid");
    assert!(valid.error.is_none());
    assert_eq!(valid.result.expect("result")["revision"], json!(1));

    let runtime_config =
        daemon.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
    assert_eq!(runtime_config["event_sink"]["enabled"], json!(true));
    assert_eq!(runtime_config["event_sink"]["allow_kinds"], json!(["webhook", "mqtt"]));
}

#[test]
fn sdk_dispatch_maps_unknown_fields_to_validation_unknown_field() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(rpc_request(
            432,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": { "profile": "desktop-full" },
                "unexpected_field": true
            }),
        ))
        .expect("negotiate");
    assert_eq!(
        response.error.expect("error").code,
        "SDK_VALIDATION_UNKNOWN_FIELD",
        "sdk requests with unknown fields should return typed validation errors"
    );
}

#[test]
fn sdk_dispatch_maps_missing_params_to_validation_invalid_argument() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(RpcRequest { id: 433, method: "sdk_shutdown_v2".to_string(), params: None })
        .expect("shutdown response");
    assert_eq!(
        response.error.expect("error").code,
        "SDK_VALIDATION_INVALID_ARGUMENT",
        "sdk requests without params should return typed validation errors"
    );
}

#[test]
fn sdk_shutdown_v2_accepts_graceful_mode() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(rpc_request(
            44,
            "sdk_shutdown_v2",
            json!({
                "mode": "graceful"
            }),
        ))
        .expect("shutdown");
    assert!(response.error.is_none());
    assert_eq!(response.result.expect("result")["accepted"], json!(true));
}

#[test]
fn sdk_snapshot_v2_returns_runtime_summary() {
    let daemon = RpcDaemon::test_instance();
    let _ = daemon.handle_rpc(rpc_request(
        9,
        "sdk_negotiate_v2",
        json!({
            "supported_contract_versions": [2],
            "requested_capabilities": [],
            "config": { "profile": "desktop-full" }
        }),
    ));

    let snapshot = daemon
        .handle_rpc(rpc_request(10, "sdk_snapshot_v2", json!({ "include_counts": true })))
        .expect("snapshot");
    assert!(snapshot.error.is_none());
    let result = snapshot.result.expect("result");
    assert_eq!(result["runtime_id"], json!("test-identity"));
    assert_eq!(result["state"], json!("running"));
    assert!(result.get("event_stream_position").is_some());
}

#[test]
fn sdk_race_cancel_and_receipt_updates_converge_to_terminal_state() {
    let daemon = RpcDaemon::test_instance();

    for idx in 0..96_u64 {
        let message_id = format!("race-message-{idx}");
        let receive = daemon
            .handle_rpc(rpc_request(
                50_000 + (idx * 10),
                "receive_message",
                json!({
                    "id": message_id,
                    "source": "race.source",
                    "destination": "race.destination",
                    "title": "",
                    "content": "race payload",
                    "fields": null
                }),
            ))
            .expect("receive");
        assert!(receive.error.is_none(), "receive_message should succeed for race setup");

        let call_cancel = |id: u64| {
            daemon
                .handle_rpc(rpc_request(
                    id,
                    "sdk_cancel_message_v2",
                    json!({ "message_id": message_id }),
                ))
                .expect("cancel")
        };
        let call_receipt = |id: u64| {
            daemon
                .handle_rpc(rpc_request(
                    id,
                    "record_receipt",
                    json!({
                        "message_id": message_id,
                        "status": "delivered"
                    }),
                ))
                .expect("record_receipt")
        };

        let cancel = if idx % 2 == 0 {
            let cancel = call_cancel(50_000 + (idx * 10) + 1);
            let receipt = call_receipt(50_000 + (idx * 10) + 2);
            assert!(receipt.error.is_none(), "record_receipt race call should stay error-free");
            cancel
        } else {
            let receipt = call_receipt(50_000 + (idx * 10) + 2);
            assert!(receipt.error.is_none(), "record_receipt race call should stay error-free");
            call_cancel(50_000 + (idx * 10) + 1)
        };

        let cancel_payload = cancel.result.expect("cancel result");
        let cancel_result = cancel_payload["result"].as_str().expect("cancel result string");
        assert!(
            matches!(cancel_result, "Accepted" | "AlreadyTerminal"),
            "cancel race should resolve to accepted or already-terminal"
        );

        let status = daemon
            .handle_rpc(rpc_request(
                50_000 + (idx * 10) + 3,
                "sdk_status_v2",
                json!({ "message_id": message_id }),
            ))
            .expect("status");
        let status_payload = status.result.expect("status result");
        let receipt_status =
            status_payload["message"]["receipt_status"].as_str().expect("status receipt_status");
        assert!(
            matches!(receipt_status, "cancelled" | "delivered"),
            "race must converge to a single terminal status"
        );

        let second_cancel = daemon
            .handle_rpc(rpc_request(
                50_000 + (idx * 10) + 4,
                "sdk_cancel_message_v2",
                json!({ "message_id": message_id }),
            ))
            .expect("second cancel");
        assert_eq!(
            second_cancel.result.expect("second cancel result")["result"],
            json!("AlreadyTerminal")
        );
    }
}
