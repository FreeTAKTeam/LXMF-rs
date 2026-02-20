#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_request(id: u64, method: &str, params: JsonValue) -> RpcRequest {
        RpcRequest { id, method: method.to_string(), params: Some(params) }
    }

    #[test]
    fn sdk_negotiate_v2_selects_contract_and_profile_limits() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                1,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [1, 2],
                    "requested_capabilities": [
                        "sdk.capability.cursor_replay",
                        "sdk.capability.async_events"
                    ],
                    "config": {
                        "profile": "desktop-local-runtime"
                    }
                }),
            ))
            .expect("negotiate should succeed");
        assert!(response.error.is_none());
        let result = response.result.expect("result");
        assert_eq!(result["active_contract_version"], json!(2));
        assert_eq!(result["contract_release"], json!("v2.5"));
        assert_eq!(result["effective_limits"]["max_poll_events"], json!(64));
    }

    #[test]
    fn sdk_negotiate_v2_fails_on_capability_overlap_miss() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                2,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.not-real"],
                    "config": { "profile": "desktop-full" }
                }),
            ))
            .expect("rpc call");
        let error = response.error.expect("must fail");
        assert_eq!(error.code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");
    }

    #[test]
    fn sdk_negotiate_v2_keeps_required_capabilities_when_optional_subset_is_requested() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                19,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.shared_instance_rpc_auth"],
                    "config": { "profile": "desktop-full" }
                }),
            ))
            .expect("rpc call");
        assert!(response.error.is_none(), "negotiation should succeed");
        let capabilities = response
            .result
            .expect("result")
            .get("effective_capabilities")
            .and_then(JsonValue::as_array)
            .cloned()
            .expect("effective capabilities");
        assert!(
            capabilities.iter().any(|value| value == "sdk.capability.shared_instance_rpc_auth"),
            "requested optional capability must be present"
        );
        assert!(
            capabilities.iter().any(|value| value == "sdk.capability.cursor_replay"),
            "required capability cursor_replay must remain present"
        );
        assert!(
            capabilities.iter().any(|value| value == "sdk.capability.config_revision_cas"),
            "required capability config_revision_cas must remain present"
        );
    }

    #[test]
    fn sdk_negotiate_v2_rejects_embedded_alloc_profile() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                20,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": { "profile": "embedded-alloc" }
                }),
            ))
            .expect("rpc call");
        let error = response.error.expect("must fail");
        assert_eq!(error.code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");
    }

    #[test]
    fn sdk_security_authorize_http_request_blocks_remote_source_in_local_only_mode() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            21,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "local_only",
                    "auth_mode": "local_trusted"
                }
            }),
        ));

        let err = daemon
            .authorize_http_request(&[], Some("10.1.2.3"))
            .expect_err("remote source should be rejected in local_only mode");
        assert_eq!(err.code, "SDK_SECURITY_REMOTE_BIND_DISALLOWED");
    }

    #[test]
    fn sdk_security_forwarded_headers_require_trusted_proxy_allowlist() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            21,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "local_only",
                    "auth_mode": "local_trusted"
                }
            }),
        ));
        let _ = daemon.handle_rpc(rpc_request(
            22,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "extensions": {
                        "trusted_proxy": true,
                        "trusted_proxy_ips": ["127.0.0.1"]
                    }
                }
            }),
        ));

        let forwarded = vec![("x-forwarded-for".to_string(), "127.0.0.1".to_string())];
        let err = daemon
            .authorize_http_request(&forwarded, Some("10.9.8.7"))
            .expect_err("untrusted proxy peer must not be able to spoof forwarded headers");
        assert_eq!(err.code, "SDK_SECURITY_REMOTE_BIND_DISALLOWED");

        daemon
            .authorize_http_request(&forwarded, Some("127.0.0.1"))
            .expect("allowlisted proxy may forward loopback source");
    }

    #[test]
    fn sdk_security_authorize_http_request_rejects_replayed_token_jti() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                22,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "token",
                        "rpc_backend": {
                            "token_auth": {
                                "issuer": "test-issuer",
                                "audience": "test-audience",
                                "jti_cache_ttl_ms": 30_000,
                                "clock_skew_ms": 0,
                                "shared_secret": "test-secret"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none());

        let iat = now_seconds_u64();
        let exp = iat.saturating_add(60);
        let payload =
            format!("iss=test-issuer;aud=test-audience;jti=token-1;sub=cli;iat={iat};exp={exp}");
        let signature =
            RpcDaemon::token_signature("test-secret", payload.as_str()).expect("token signature");
        let token = format!("{payload};sig={signature}");
        let headers = vec![("authorization".to_string(), format!("Bearer {token}"))];
        daemon.authorize_http_request(&headers, Some("10.5.6.7")).expect("first token should pass");
        let replay = daemon
            .authorize_http_request(&headers, Some("10.5.6.7"))
            .expect_err("replayed token jti should be rejected");
        assert_eq!(replay.code, "SDK_SECURITY_TOKEN_REPLAYED");
    }

    #[test]
    fn sdk_security_authorize_http_request_rejects_invalid_token_signature_and_expiry() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                23,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "token",
                        "rpc_backend": {
                            "token_auth": {
                                "issuer": "test-issuer",
                                "audience": "test-audience",
                                "jti_cache_ttl_ms": 30_000,
                                "clock_skew_ms": 0,
                                "shared_secret": "test-secret"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none());

        let now = now_seconds_u64();
        let expired_payload = format!(
            "iss=test-issuer;aud=test-audience;jti=expired-1;sub=cli;iat={};exp={}",
            now.saturating_sub(120),
            now.saturating_sub(60)
        );
        let expired_sig = RpcDaemon::token_signature("test-secret", expired_payload.as_str())
            .expect("token signature");
        let expired_headers = vec![(
            "authorization".to_string(),
            format!("Bearer {expired_payload};sig={expired_sig}"),
        )];
        let expired = daemon
            .authorize_http_request(&expired_headers, Some("10.5.6.7"))
            .expect_err("expired token should be rejected");
        assert_eq!(expired.code, "SDK_SECURITY_TOKEN_INVALID");

        let valid_payload = format!(
            "iss=test-issuer;aud=test-audience;jti=tampered-1;sub=cli;iat={now};exp={}",
            now.saturating_add(60)
        );
        let tampered_headers =
            vec![("authorization".to_string(), format!("Bearer {valid_payload};sig=deadbeef"))];
        let tampered = daemon
            .authorize_http_request(&tampered_headers, Some("10.5.6.7"))
            .expect_err("tampered signature should be rejected");
        assert_eq!(tampered.code, "SDK_SECURITY_TOKEN_INVALID");
    }

    #[test]
    fn sdk_negotiate_v2_accepts_mtls_auth_mode_with_backend_config() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                24,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.mtls_auth"],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "mtls",
                        "rpc_backend": {
                            "mtls_auth": {
                                "ca_bundle_path": "/tmp/test-ca.pem",
                                "require_client_cert": true,
                                "allowed_san": "urn:test-san"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none(), "mtls negotiation should succeed");
        let result = response.result.expect("result");
        let capabilities =
            result["effective_capabilities"].as_array().expect("effective_capabilities");
        assert!(
            capabilities.iter().any(|capability| capability == "sdk.capability.mtls_auth"),
            "mtls capability should be advertised after mtls negotiation"
        );
    }

    #[test]
    fn sdk_security_authorize_http_request_enforces_mtls_headers_and_policy() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                25,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "mtls",
                        "rpc_backend": {
                            "mtls_auth": {
                                "ca_bundle_path": "/tmp/test-ca.pem",
                                "require_client_cert": true,
                                "allowed_san": "urn:test-san"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none());

        let missing_cert = daemon
            .authorize_http_request(&[], Some("10.5.6.7"))
            .expect_err("missing mtls cert header should be rejected");
        assert_eq!(missing_cert.code, "SDK_SECURITY_AUTH_REQUIRED");

        let wrong_san_headers = vec![
            ("x-client-cert-present".to_string(), "1".to_string()),
            ("x-client-san".to_string(), "urn:wrong-san".to_string()),
        ];
        let wrong_san = daemon
            .authorize_http_request(&wrong_san_headers, Some("10.5.6.7"))
            .expect_err("non-matching mtls SAN should be rejected");
        assert_eq!(wrong_san.code, "SDK_SECURITY_AUTHZ_DENIED");

        let valid_headers = vec![
            ("x-client-cert-present".to_string(), "1".to_string()),
            ("x-client-san".to_string(), "urn:test-san".to_string()),
            ("x-client-subject".to_string(), "sdk-client-mtls".to_string()),
        ];
        daemon
            .authorize_http_request(&valid_headers, Some("10.5.6.7"))
            .expect("valid mtls headers should authorize request");
    }

    #[test]
    fn sdk_security_authorize_http_request_enforces_rate_limits_and_emits_event() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            23,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "local_only",
                    "auth_mode": "local_trusted"
                }
            }),
        ));
        let _ = daemon.handle_rpc(rpc_request(
            24,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "extensions": {
                        "rate_limits": {
                            "per_ip_per_minute": 1,
                            "per_principal_per_minute": 1
                        }
                    }
                }
            }),
        ));

        daemon.authorize_http_request(&[], Some("127.0.0.1")).expect("first request should pass");
        let limited = daemon
            .authorize_http_request(&[], Some("127.0.0.1"))
            .expect_err("second request should be rate limited");
        assert_eq!(limited.code, "SDK_SECURITY_RATE_LIMITED");

        let mut found_security_event = false;
        for _ in 0..8 {
            let Some(event) = daemon.take_event() else {
                break;
            };
            if event.event_type == "sdk_security_rate_limited" {
                found_security_event = true;
                break;
            }
        }
        assert!(found_security_event, "rate-limit violations should emit security event");
    }

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

    #[test]
    fn sdk_release_b_domain_methods_roundtrip() {
        let daemon = RpcDaemon::test_instance();

        let topic = daemon
            .handle_rpc(rpc_request(
                90,
                "sdk_topic_create_v2",
                json!({
                    "topic_path": "ops/alerts",
                    "metadata": { "kind": "ops" },
                    "extensions": { "scope": "test" }
                }),
            ))
            .expect("topic create");
        assert!(topic.error.is_none());
        let topic_id = topic.result.expect("topic result")["topic"]["topic_id"]
            .as_str()
            .expect("topic id")
            .to_string();

        let topic_get = daemon
            .handle_rpc(rpc_request(
                91,
                "sdk_topic_get_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("topic get");
        assert!(topic_get.error.is_none());
        assert_eq!(topic_get.result.expect("result")["topic"]["topic_path"], json!("ops/alerts"));

        let topic_list = daemon
            .handle_rpc(rpc_request(92, "sdk_topic_list_v2", json!({ "limit": 10 })))
            .expect("topic list");
        assert!(topic_list.error.is_none());
        assert_eq!(
            topic_list.result.expect("result")["topics"].as_array().expect("topic array").len(),
            1
        );

        let topic_subscribe = daemon
            .handle_rpc(rpc_request(
                93,
                "sdk_topic_subscribe_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("topic subscribe");
        assert!(topic_subscribe.error.is_none());
        assert_eq!(topic_subscribe.result.expect("result")["accepted"], json!(true));

        let publish = daemon
            .handle_rpc(rpc_request(
                94,
                "sdk_topic_publish_v2",
                json!({
                    "topic_id": topic_id.clone(),
                    "payload": { "message": "hello topic" },
                    "correlation_id": "corr-1"
                }),
            ))
            .expect("topic publish");
        assert!(publish.error.is_none());
        assert_eq!(publish.result.expect("result")["accepted"], json!(true));

        let telemetry = daemon
            .handle_rpc(rpc_request(
                95,
                "sdk_telemetry_query_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("telemetry query");
        assert!(telemetry.error.is_none());
        assert!(!telemetry.result.expect("result")["points"]
            .as_array()
            .expect("points array")
            .is_empty());

        let attachment = daemon
            .handle_rpc(rpc_request(
                96,
                "sdk_attachment_store_v2",
                json!({
                    "name": "sample.txt",
                    "content_type": "text/plain",
                    "bytes_base64": "aGVsbG8gd29ybGQ=",
                    "topic_ids": [topic_id.clone()]
                }),
            ))
            .expect("attachment store");
        assert!(attachment.error.is_none());
        let attachment_id = attachment.result.expect("result")["attachment"]["attachment_id"]
            .as_str()
            .expect("attachment id")
            .to_string();

        let attachment_get = daemon
            .handle_rpc(rpc_request(
                97,
                "sdk_attachment_get_v2",
                json!({ "attachment_id": attachment_id }),
            ))
            .expect("attachment get");
        assert!(attachment_get.error.is_none());
        assert_eq!(
            attachment_get.result.expect("result")["attachment"]["name"],
            json!("sample.txt")
        );

        let attachment_list = daemon
            .handle_rpc(rpc_request(
                98,
                "sdk_attachment_list_v2",
                json!({ "topic_id": topic_id.clone() }),
            ))
            .expect("attachment list");
        assert!(attachment_list.error.is_none());
        assert_eq!(
            attachment_list.result.expect("result")["attachments"]
                .as_array()
                .expect("attachments array")
                .len(),
            1
        );

        let marker = daemon
            .handle_rpc(rpc_request(
                99,
                "sdk_marker_create_v2",
                json!({
                    "label": "Alpha",
                    "position": { "lat": 35.0, "lon": -115.0, "alt_m": 1200.0 },
                    "topic_id": topic_id.clone()
                }),
            ))
            .expect("marker create");
        assert!(marker.error.is_none());
        let marker_id = marker.result.expect("result")["marker"]["marker_id"]
            .as_str()
            .expect("marker id")
            .to_string();

        let marker_update = daemon
            .handle_rpc(rpc_request(
                100,
                "sdk_marker_update_position_v2",
                json!({
                    "marker_id": marker_id,
                    "position": { "lat": 36.0, "lon": -116.0, "alt_m": null }
                }),
            ))
            .expect("marker update");
        assert!(marker_update.error.is_none());
        assert_eq!(marker_update.result.expect("result")["marker"]["position"]["lat"], json!(36.0));
    }

    #[test]
    fn sdk_release_b_filtered_list_cursor_does_not_stall_on_no_matches() {
        let daemon = RpcDaemon::test_instance();
        let topic_a = daemon
            .handle_rpc(rpc_request(110, "sdk_topic_create_v2", json!({ "topic_path": "ops/a" })))
            .expect("topic a");
        let topic_b = daemon
            .handle_rpc(rpc_request(111, "sdk_topic_create_v2", json!({ "topic_path": "ops/b" })))
            .expect("topic b");
        let topic_a_id = topic_a.result.expect("result")["topic"]["topic_id"]
            .as_str()
            .expect("topic_a_id")
            .to_string();
        let topic_b_id = topic_b.result.expect("result")["topic"]["topic_id"]
            .as_str()
            .expect("topic_b_id")
            .to_string();

        let _ = daemon
            .handle_rpc(rpc_request(
                112,
                "sdk_attachment_store_v2",
                json!({
                    "name": "a.bin",
                    "content_type": "application/octet-stream",
                    "bytes_base64": "AA==",
                    "topic_ids": [topic_a_id.clone()]
                }),
            ))
            .expect("attachment store");
        let _ = daemon
            .handle_rpc(rpc_request(
                113,
                "sdk_marker_create_v2",
                json!({
                    "label": "A",
                    "position": { "lat": 1.0, "lon": 1.0, "alt_m": null },
                    "topic_id": topic_a_id
                }),
            ))
            .expect("marker create");

        let attachment_list = daemon
            .handle_rpc(rpc_request(
                114,
                "sdk_attachment_list_v2",
                json!({ "topic_id": topic_b_id.clone(), "cursor": null, "limit": 10 }),
            ))
            .expect("attachment list");
        assert!(attachment_list.error.is_none());
        let attachment_result = attachment_list.result.expect("attachment list result");
        assert_eq!(attachment_result["attachments"], json!([]));
        assert_eq!(attachment_result["next_cursor"], JsonValue::Null);

        let marker_list = daemon
            .handle_rpc(rpc_request(
                115,
                "sdk_marker_list_v2",
                json!({ "topic_id": topic_b_id, "cursor": null, "limit": 10 }),
            ))
            .expect("marker list");
        assert!(marker_list.error.is_none());
        let marker_result = marker_list.result.expect("marker list result");
        assert_eq!(marker_result["markers"], json!([]));
        assert_eq!(marker_result["next_cursor"], JsonValue::Null);
    }

    #[test]
    fn sdk_release_c_domain_methods_roundtrip() {
        let daemon = RpcDaemon::test_instance();
        let list_before =
            daemon.handle_rpc(rpc_request(120, "sdk_identity_list_v2", json!({}))).expect("list");
        assert!(list_before.error.is_none());
        assert!(!list_before.result.expect("result")["identities"]
            .as_array()
            .expect("identity array")
            .is_empty());

        let identity_bundle = json!({
            "identity": "node-b",
            "public_key": "node-b-pub",
            "display_name": "Node B",
            "capabilities": ["ops"],
            "extensions": {}
        });
        let identity_import = daemon
            .handle_rpc(rpc_request(
                121,
                "sdk_identity_import_v2",
                json!({
                    "bundle_base64": BASE64_STANDARD.encode(identity_bundle.to_string().as_bytes()),
                    "passphrase": null
                }),
            ))
            .expect("identity import");
        assert!(identity_import.error.is_none());
        assert_eq!(
            identity_import.result.expect("result")["identity"]["identity"],
            json!("node-b")
        );

        let identity_resolve = daemon
            .handle_rpc(rpc_request(
                122,
                "sdk_identity_resolve_v2",
                json!({ "hash": "node-b-pub" }),
            ))
            .expect("identity resolve");
        assert!(identity_resolve.error.is_none());
        assert_eq!(identity_resolve.result.expect("result")["identity"], json!("node-b"));

        let identity_export = daemon
            .handle_rpc(rpc_request(123, "sdk_identity_export_v2", json!({ "identity": "node-b" })))
            .expect("identity export");
        assert!(identity_export.error.is_none());
        assert!(!identity_export.result.expect("result")["bundle"]["bundle_base64"]
            .as_str()
            .expect("export bundle")
            .is_empty());

        let _ = daemon
            .handle_rpc(rpc_request(
                124,
                "send_message_v2",
                json!({
                    "id": "paper-msg-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "paper body"
                }),
            ))
            .expect("send message for paper");
        let paper_encode = daemon
            .handle_rpc(rpc_request(
                125,
                "sdk_paper_encode_v2",
                json!({ "message_id": "paper-msg-1" }),
            ))
            .expect("paper encode");
        assert!(paper_encode.error.is_none());
        let uri = paper_encode.result.expect("result")["envelope"]["uri"]
            .as_str()
            .expect("paper uri")
            .to_string();
        assert!(uri.starts_with("lxm://"));

        let paper_decode = daemon
            .handle_rpc(rpc_request(126, "sdk_paper_decode_v2", json!({ "uri": uri })))
            .expect("paper decode");
        assert!(paper_decode.error.is_none());
        assert_eq!(paper_decode.result.expect("result")["accepted"], json!(true));

        let command = daemon
            .handle_rpc(rpc_request(
                127,
                "sdk_command_invoke_v2",
                json!({
                    "command": "ping",
                    "target": "node-b",
                    "payload": { "body": "hello" },
                    "timeout_ms": 1000
                }),
            ))
            .expect("command invoke");
        assert!(command.error.is_none());
        let correlation_id = command.result.expect("result")["response"]["payload"]
            ["correlation_id"]
            .as_str()
            .expect("correlation id")
            .to_string();

        let command_reply = daemon
            .handle_rpc(rpc_request(
                128,
                "sdk_command_reply_v2",
                json!({
                    "correlation_id": correlation_id,
                    "accepted": true,
                    "payload": { "reply": "pong" }
                }),
            ))
            .expect("command reply");
        assert!(command_reply.error.is_none());
        assert_eq!(command_reply.result.expect("result")["accepted"], json!(true));

        let voice_open = daemon
            .handle_rpc(rpc_request(
                129,
                "sdk_voice_session_open_v2",
                json!({ "peer_id": "node-b", "codec_hint": "opus" }),
            ))
            .expect("voice open");
        assert!(voice_open.error.is_none());
        let session_id = voice_open.result.expect("result")["session_id"]
            .as_str()
            .expect("session id")
            .to_string();

        let voice_update = daemon
            .handle_rpc(rpc_request(
                130,
                "sdk_voice_session_update_v2",
                json!({ "session_id": session_id.clone(), "state": "active" }),
            ))
            .expect("voice update");
        assert!(voice_update.error.is_none());
        assert_eq!(voice_update.result.expect("result")["state"], json!("active"));

        let voice_close = daemon
            .handle_rpc(rpc_request(
                131,
                "sdk_voice_session_close_v2",
                json!({ "session_id": session_id }),
            ))
            .expect("voice close");
        assert!(voice_close.error.is_none());
        assert_eq!(voice_close.result.expect("result")["accepted"], json!(true));
    }

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
}
