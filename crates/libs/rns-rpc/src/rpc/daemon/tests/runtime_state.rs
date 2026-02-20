    #[test]
    fn sdk_cancel_message_v2_distinguishes_not_found_and_too_late() {
        let daemon = RpcDaemon::test_instance();

        let not_found = daemon
            .handle_rpc(rpc_request(6, "sdk_cancel_message_v2", json!({ "message_id": "missing" })))
            .expect("cancel missing");
        assert_eq!(not_found.result.expect("result")["result"], json!("NotFound"));

        let send = daemon
            .handle_rpc(rpc_request(
                7,
                "send_message_v2",
                json!({
                    "id": "outbound-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");
        assert!(send.error.is_none());

        let too_late = daemon
            .handle_rpc(rpc_request(
                8,
                "sdk_cancel_message_v2",
                json!({ "message_id": "outbound-1" }),
            ))
            .expect("cancel");
        assert_eq!(too_late.result.expect("result")["result"], json!("TooLateToCancel"));
    }

    #[test]
    fn sdk_status_v2_returns_message_record() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon
            .handle_rpc(rpc_request(
                40,
                "send_message_v2",
                json!({
                    "id": "status-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");
        let response = daemon
            .handle_rpc(rpc_request(
                41,
                "sdk_status_v2",
                json!({
                    "message_id": "status-1"
                }),
            ))
            .expect("status");
        assert_eq!(response.result.expect("result")["message"]["id"], json!("status-1"));
    }

    #[test]
    fn sdk_property_terminal_receipt_status_is_sticky() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon
            .handle_rpc(rpc_request(
                45,
                "send_message_v2",
                json!({
                    "id": "property-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");

        let delivered = daemon
            .handle_rpc(rpc_request(
                46,
                "record_receipt",
                json!({
                    "message_id": "property-1",
                    "status": "delivered"
                }),
            ))
            .expect("record delivered");
        assert_eq!(delivered.result.expect("result")["updated"], json!(true));
        let trace_before = daemon
            .handle_rpc(rpc_request(
                460,
                "message_delivery_trace",
                json!({
                    "message_id": "property-1"
                }),
            ))
            .expect("trace before ignored update");
        let trace_before_len = trace_before.result.expect("result")["transitions"]
            .as_array()
            .expect("trace entries")
            .len();

        let ignored = daemon
            .handle_rpc(rpc_request(
                47,
                "record_receipt",
                json!({
                    "message_id": "property-1",
                    "status": "sent: direct"
                }),
            ))
            .expect("record after terminal");
        let ignored_result = ignored.result.expect("result");
        assert_eq!(ignored_result["updated"], json!(false));
        assert_eq!(ignored_result["status"], json!("delivered"));
        let trace_after = daemon
            .handle_rpc(rpc_request(
                470,
                "message_delivery_trace",
                json!({
                    "message_id": "property-1"
                }),
            ))
            .expect("trace after ignored update");
        let trace_after_len = trace_after.result.expect("result")["transitions"]
            .as_array()
            .expect("trace entries")
            .len();
        assert_eq!(
            trace_after_len, trace_before_len,
            "ignored terminal updates must not append delivery trace entries"
        );

        let status = daemon
            .handle_rpc(rpc_request(
                48,
                "sdk_status_v2",
                json!({
                    "message_id": "property-1"
                }),
            ))
            .expect("status");
        assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("delivered"));
    }

    #[test]
    fn sdk_property_event_sequence_is_monotonic() {
        let daemon = RpcDaemon::test_instance();
        daemon.emit_event(RpcEvent {
            event_type: "property".to_string(),
            payload: json!({ "idx": 1 }),
        });
        daemon.emit_event(RpcEvent {
            event_type: "property".to_string(),
            payload: json!({ "idx": 2 }),
        });

        let response = daemon
            .handle_rpc(rpc_request(
                49,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 2
                }),
            ))
            .expect("poll");
        let events =
            response.result.expect("result")["events"].as_array().expect("events array").to_vec();
        assert_eq!(events.len(), 2);
        let first = events[0]["seq_no"].as_u64().expect("first seq");
        let second = events[1]["seq_no"].as_u64().expect("second seq");
        assert!(second > first, "event sequence must be strictly increasing");
    }

    #[test]
    fn sdk_property_cursor_churn_keeps_monotonic_progress() {
        let daemon = RpcDaemon::test_instance();
        for idx in 0..96_u64 {
            daemon.emit_event(RpcEvent {
                event_type: "property_churn".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let mut cursor: Option<String> = None;
        let mut last_seq = 0_u64;
        let mut seen = HashSet::new();

        for iteration in 0..256_u64 {
            let response = daemon
                .handle_rpc(rpc_request(
                    5_000 + iteration,
                    "sdk_poll_events_v2",
                    json!({
                        "cursor": cursor.clone(),
                        "max": ((iteration % 7) + 1) as usize,
                    }),
                ))
                .expect("poll");
            assert!(response.error.is_none(), "poll should remain stable under churn");
            let result = response.result.expect("result");
            let events = result["events"].as_array().expect("events array");
            for event in events {
                let seq = event["seq_no"].as_u64().expect("sequence number");
                assert!(seq > last_seq, "sequence must be strictly increasing");
                assert!(seen.insert(seq), "sequence IDs must not repeat");
                last_seq = seq;
            }

            cursor = result["next_cursor"].as_str().map(ToOwned::to_owned);
            if seen.len() >= 96 {
                break;
            }
        }

        assert_eq!(
            seen.len(),
            96,
            "variable-batch polling should consume each emitted event exactly once"
        );
    }

    #[test]
    fn sdk_configure_v2_applies_revision_cas() {
        let daemon = RpcDaemon::test_instance();
        let first = daemon
            .handle_rpc(rpc_request(
                42,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": { "event_stream": { "max_poll_events": 64 } }
                }),
            ))
            .expect("configure");
        assert_eq!(first.result.expect("result")["revision"], json!(1));

        let conflict = daemon
            .handle_rpc(rpc_request(
                43,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": { "event_stream": { "max_poll_events": 32 } }
                }),
            ))
            .expect("configure conflict");
        assert_eq!(conflict.error.expect("error").code, "SDK_CONFIG_CONFLICT");
    }

    #[test]
    fn sdk_configure_v2_validates_patch_before_commit_and_revision_bump() {
        let daemon = RpcDaemon::test_instance();
        let invalid = daemon
            .handle_rpc(rpc_request(
                430,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": { "overflow_policy": "block" }
                }),
            ))
            .expect("configure invalid patch");
        assert_eq!(
            invalid.error.expect("error").code,
            "SDK_VALIDATION_INVALID_ARGUMENT",
            "invalid patch should fail before config commit"
        );

        let valid = daemon
            .handle_rpc(rpc_request(
                431,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": { "event_stream": { "max_poll_events": 64 } }
                }),
            ))
            .expect("configure valid patch");
        assert_eq!(
            valid.result.expect("result")["revision"],
            json!(1),
            "failed patch must not consume config revision"
        );
    }

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
        assert_eq!(
            below_min_batch.error.expect("error").code,
            "SDK_VALIDATION_INVALID_ARGUMENT"
        );

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
            .handle_rpc(RpcRequest {
                id: 433,
                method: "sdk_shutdown_v2".to_string(),
                params: None,
            })
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
