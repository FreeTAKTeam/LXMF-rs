    #[test]
    fn sdk_poll_events_v2_validates_cursor_and_expires_stale_tokens() {
        let daemon = RpcDaemon::test_instance();
        daemon.emit_event(RpcEvent {
            event_type: "inbound".to_string(),
            payload: json!({ "message_id": "m-1" }),
        });
        let first = daemon
            .handle_rpc(rpc_request(
                3,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 4
                }),
            ))
            .expect("poll");
        let first_result = first.result.expect("result");
        let cursor = first_result["next_cursor"].as_str().expect("cursor").to_string();
        assert!(first_result["events"].as_array().is_some_and(|events| !events.is_empty()));

        let invalid = daemon
            .handle_rpc(rpc_request(
                4,
                "sdk_poll_events_v2",
                json!({
                    "cursor": "bad-cursor",
                    "max": 4
                }),
            ))
            .expect("invalid poll should still return response");
        assert_eq!(invalid.error.expect("error").code, "SDK_RUNTIME_INVALID_CURSOR");

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 8) {
            daemon.emit_event(RpcEvent {
                event_type: "inbound".to_string(),
                payload: json!({ "message_id": format!("overflow-{idx}") }),
            });
        }

        let expired = daemon
            .handle_rpc(rpc_request(
                5,
                "sdk_poll_events_v2",
                json!({
                    "cursor": cursor,
                    "max": 2
                }),
            ))
            .expect("expired poll should return response");
        assert_eq!(expired.error.expect("error").code, "SDK_RUNTIME_CURSOR_EXPIRED");
    }

    #[test]
    fn sdk_poll_events_v2_requires_successful_reset_after_degraded_state() {
        let daemon = RpcDaemon::test_instance();
        daemon.emit_event(RpcEvent { event_type: "inbound".to_string(), payload: json!({}) });
        let first = daemon
            .handle_rpc(rpc_request(
                30,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 1
                }),
            ))
            .expect("initial poll");
        let cursor =
            first.result.expect("result")["next_cursor"].as_str().expect("cursor").to_string();

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 4) {
            daemon.emit_event(RpcEvent {
                event_type: "inbound".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let expired = daemon
            .handle_rpc(rpc_request(
                31,
                "sdk_poll_events_v2",
                json!({
                    "cursor": cursor,
                    "max": 1
                }),
            ))
            .expect("expired");
        assert_eq!(expired.error.expect("error").code, "SDK_RUNTIME_CURSOR_EXPIRED");

        let invalid_reset = daemon
            .handle_rpc(rpc_request(
                32,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 0
                }),
            ))
            .expect("invalid reset");
        assert_eq!(invalid_reset.error.expect("error").code, "SDK_VALIDATION_INVALID_ARGUMENT");

        let still_degraded = daemon
            .handle_rpc(rpc_request(
                33,
                "sdk_poll_events_v2",
                json!({
                    "cursor": "v2:test-identity:sdk-events:999999",
                    "max": 1
                }),
            ))
            .expect("still degraded");
        assert_eq!(still_degraded.error.expect("error").code, "SDK_RUNTIME_STREAM_DEGRADED");

        let reset_ok = daemon
            .handle_rpc(rpc_request(
                34,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 1
                }),
            ))
            .expect("reset");
        assert!(reset_ok.error.is_none());
    }

    #[test]
    fn sdk_send_v2_persists_outbound_message() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                5,
                "sdk_send_v2",
                json!({
                    "id": "sdk-send-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("sdk_send_v2");
        assert!(response.error.is_none());
        assert_eq!(response.result.expect("result")["message_id"], json!("sdk-send-1"));
    }

    #[test]
    fn sdk_domain_methods_respect_capability_gating_when_removed() {
        let daemon = RpcDaemon::test_instance();
        {
            let mut capabilities = daemon
                .sdk_effective_capabilities
                .lock()
                .expect("sdk_effective_capabilities mutex poisoned");
            *capabilities = vec!["sdk.capability.cursor_replay".to_string()];
        }
        let response = daemon
            .handle_rpc(rpc_request(
                77,
                "sdk_topic_create_v2",
                json!({ "topic_path": "ops/alpha" }),
            ))
            .expect("rpc response");
        let error = response.error.expect("expected capability error");
        assert_eq!(error.code, "SDK_CAPABILITY_DISABLED");
        assert!(error.message.contains("sdk_topic_create_v2"));
    }

