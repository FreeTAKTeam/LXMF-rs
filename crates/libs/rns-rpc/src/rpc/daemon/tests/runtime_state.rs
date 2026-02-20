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
