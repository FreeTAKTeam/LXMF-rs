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
    fn sdk_negotiate_v2_falls_back_to_n_when_future_versions_are_advertised() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                11,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [4, 3, 2],
                    "requested_capabilities": [],
                    "config": { "profile": "desktop-full" }
                }),
            ))
            .expect("negotiate should succeed");
        assert!(response.error.is_none(), "negotiation should fall back to contract N");
        let result = response.result.expect("result");
        assert_eq!(result["active_contract_version"], json!(2));
    }

    #[test]
    fn sdk_negotiate_v2_rejects_when_only_future_versions_are_present() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                12,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [4, 3],
                    "requested_capabilities": [],
                    "config": { "profile": "desktop-full" }
                }),
            ))
            .expect("rpc call");
        let error = response.error.expect("must fail");
        assert_eq!(error.code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");
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
    fn sdk_negotiate_v2_ignores_unknown_capabilities_when_overlap_exists() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                23,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [
                        "sdk.capability.shared_instance_rpc_auth",
                        "sdk.capability.future_contract_extension"
                    ],
                    "config": { "profile": "desktop-full" }
                }),
            ))
            .expect("rpc call");
        assert!(response.error.is_none(), "known overlap should negotiate successfully");
        let capabilities = response
            .result
            .expect("result")
            .get("effective_capabilities")
            .and_then(JsonValue::as_array)
            .cloned()
            .expect("effective capabilities");
        assert!(
            capabilities.iter().any(|value| value == "sdk.capability.shared_instance_rpc_auth"),
            "known requested capability must be preserved"
        );
        assert!(
            !capabilities
                .iter()
                .any(|value| value == "sdk.capability.future_contract_extension"),
            "unknown capability must be ignored, not echoed into effective set"
        );
    }

    #[test]
    fn sdk_negotiate_v2_accepts_embedded_alloc_profile_with_reduced_limits() {
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
        assert!(response.error.is_none(), "embedded profile should negotiate");
        let result = response.result.expect("result");
        assert_eq!(result["effective_limits"]["max_poll_events"], json!(32));
        let capabilities =
            result["effective_capabilities"].as_array().expect("effective_capabilities");
        assert!(
            !capabilities.iter().any(|capability| capability == "sdk.capability.async_events"),
            "embedded profile must not advertise async_events"
        );
        assert!(
            capabilities.iter().any(|capability| capability == "sdk.capability.manual_tick"),
            "embedded profile must advertise manual_tick capability"
        );
    }

    #[test]
    fn sdk_negotiate_v2_rejects_mtls_for_embedded_alloc_profile() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                20,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": {
                        "profile": "embedded-alloc",
                        "bind_mode": "remote",
                        "auth_mode": "mtls",
                        "rpc_backend": {
                            "mtls_auth": {
                                "ca_bundle_path": "/tmp/test-ca.pem",
                                "require_client_cert": false
                            }
                        }
                    }
                }),
            ))
            .expect("rpc call");
        let error = response.error.expect("must fail");
        assert_eq!(error.code, "SDK_VALIDATION_INVALID_ARGUMENT");
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
                                "allowed_san": "urn:test-san",
                                "client_cert_path": "/tmp/test-client.pem",
                                "client_key_path": "/tmp/test-client.key"
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
    fn sdk_security_authorize_http_request_enforces_mtls_transport_context_and_policy() {
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
                                "allowed_san": "urn:test-san",
                                "client_cert_path": "/tmp/test-client.pem",
                                "client_key_path": "/tmp/test-client.key"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none());

        let spoofed_headers = vec![
            ("x-client-cert-present".to_string(), "1".to_string()),
            ("x-client-san".to_string(), "urn:test-san".to_string()),
        ];
        let spoofed = daemon
            .authorize_http_request(&spoofed_headers, Some("10.5.6.7"))
            .expect_err("legacy mtls headers must not bypass transport-auth checks");
        assert_eq!(spoofed.code, "SDK_SECURITY_AUTH_REQUIRED");

        let missing_transport_context = daemon
            .authorize_http_request_with_transport(&[], Some("10.5.6.7"), None)
            .expect_err("missing tls transport context should be rejected");
        assert_eq!(missing_transport_context.code, "SDK_SECURITY_AUTH_REQUIRED");

        let missing_cert_context = crate::rpc::http::TransportAuthContext::default();
        let missing_cert = daemon
            .authorize_http_request_with_transport(
                &[],
                Some("10.5.6.7"),
                Some(&missing_cert_context),
            )
            .expect_err("missing mtls cert in transport context should be rejected");
        assert_eq!(missing_cert.code, "SDK_SECURITY_AUTH_REQUIRED");

        let wrong_san_context = crate::rpc::http::TransportAuthContext {
            client_cert_present: true,
            client_subject: Some("sdk-client-mtls".to_string()),
            client_sans: vec!["urn:wrong-san".to_string()],
        };
        let wrong_san = daemon
            .authorize_http_request_with_transport(&[], Some("10.5.6.7"), Some(&wrong_san_context))
            .expect_err("non-matching mtls SAN should be rejected");
        assert_eq!(wrong_san.code, "SDK_SECURITY_AUTHZ_DENIED");

        let valid_context = crate::rpc::http::TransportAuthContext {
            client_cert_present: true,
            client_subject: Some("sdk-client-mtls".to_string()),
            client_sans: vec!["urn:test-san".to_string()],
        };
        daemon
            .authorize_http_request_with_transport(&[], Some("10.5.6.7"), Some(&valid_context))
            .expect("valid mtls transport context should authorize request");
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
    fn sdk_security_events_redact_sensitive_fields_by_default() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            26,
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

        let configure = daemon
            .handle_rpc(rpc_request(
                27,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "rpc_backend": {
                            "token_auth": {
                                "issuer": "test-issuer",
                                "audience": "test-audience",
                                "jti_cache_ttl_ms": 60000,
                                "clock_skew_ms": 0,
                                "shared_secret": "top-secret-token"
                            }
                        }
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none(), "configure should succeed");

        let mut redacted_value = None;
        for _ in 0..8 {
            let Some(event) = daemon.take_event() else {
                break;
            };
            if event.event_type == "config_updated" {
                redacted_value = event
                    .payload
                    .get("patch")
                    .and_then(|value| value.get("rpc_backend"))
                    .and_then(|value| value.get("token_auth"))
                    .and_then(|value| value.get("shared_secret"))
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned);
                break;
            }
        }

        let redacted_value = redacted_value.expect("config_updated event should include shared_secret");
        assert_ne!(redacted_value, "top-secret-token");
        assert!(
            redacted_value.starts_with("sha256:"),
            "default redaction transform should hash sensitive values"
        );
    }

    #[test]
    fn sdk_security_rate_limit_event_redacts_source_ip_and_principal() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            28,
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
            29,
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
        let _ = daemon
            .authorize_http_request(&[], Some("127.0.0.1"))
            .expect_err("second request should be rate limited");

        let mut source_ip = None;
        let mut principal = None;
        for _ in 0..8 {
            let Some(event) = daemon.take_event() else {
                break;
            };
            if event.event_type == "sdk_security_rate_limited" {
                source_ip =
                    event.payload.get("source_ip").and_then(JsonValue::as_str).map(str::to_owned);
                principal =
                    event.payload.get("principal").and_then(JsonValue::as_str).map(str::to_owned);
                break;
            }
        }

        let source_ip = source_ip.expect("security event should include redacted source_ip");
        let principal = principal.expect("security event should include redacted principal");
        assert_ne!(source_ip, "127.0.0.1");
        assert_ne!(principal, "local");
        assert!(source_ip.starts_with("sha256:"));
        assert!(principal.starts_with("sha256:"));
    }

    #[test]
    fn sdk_lifecycle_traces_include_correlation_fields() {
        let daemon = RpcDaemon::test_instance();

        let configure = daemon
            .handle_rpc(rpc_request(
                40,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "event_stream": { "max_poll_events": 16 }
                    }
                }),
            ))
            .expect("configure");
        assert!(configure.error.is_none());

        let shutdown = daemon
            .handle_rpc(rpc_request(
                41,
                "sdk_shutdown_v2",
                json!({
                    "mode": "graceful",
                    "flush_timeout_ms": 50
                }),
            ))
            .expect("shutdown");
        assert!(shutdown.error.is_none());

        let mut found_config_finish = false;
        let mut found_shutdown_finish = false;
        for _ in 0..24 {
            let Some(event) = daemon.take_event() else {
                break;
            };
            if event.event_type != "sdk_lifecycle_trace" {
                continue;
            }
            let method = event.payload.get("method").and_then(JsonValue::as_str).unwrap_or("");
            let phase = event.payload.get("phase").and_then(JsonValue::as_str).unwrap_or("");
            let trace_ref = event.payload.get("trace_ref").and_then(JsonValue::as_str).unwrap_or("");
            assert!(
                trace_ref.starts_with("ref-"),
                "trace_ref should provide a stable non-secret correlation handle"
            );

            if method == "sdk_configure_v2" && phase == "finish" {
                found_config_finish = true;
                assert!(event
                    .payload
                    .get("details")
                    .and_then(|details| details.get("revision"))
                    .and_then(JsonValue::as_u64)
                    .is_some());
                assert!(event
                    .payload
                    .get("details")
                    .and_then(|details| details.get("error_code"))
                    .is_none());
            }
            if method == "sdk_shutdown_v2" && phase == "finish" {
                found_shutdown_finish = true;
                assert!(event
                    .payload
                    .get("details")
                    .and_then(|details| details.get("mode"))
                    .and_then(JsonValue::as_str)
                    .is_some_and(|mode| mode == "graceful"));
                assert!(event
                    .payload
                    .get("details")
                    .and_then(|details| details.get("error_code"))
                    .is_none());
            }
        }
        assert!(found_config_finish, "configure should emit lifecycle finish trace");
        assert!(found_shutdown_finish, "shutdown should emit lifecycle finish trace");
    }

    #[test]
    fn sdk_lifecycle_trace_redacts_sensitive_trace_id() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            42,
            "sdk_shutdown_v2",
            json!({
                "mode": "graceful",
                "flush_timeout_ms": 10
            }),
        ));

        let mut trace_id = None;
        let mut trace_ref = None;
        for _ in 0..16 {
            let Some(event) = daemon.take_event() else {
                break;
            };
            if event.event_type != "sdk_lifecycle_trace" {
                continue;
            }
            if event.payload.get("method").and_then(JsonValue::as_str)
                == Some("sdk_shutdown_v2")
                && event.payload.get("phase").and_then(JsonValue::as_str) == Some("finish")
            {
                trace_id = event
                    .payload
                    .get("trace_id")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned);
                trace_ref = event
                    .payload
                    .get("trace_ref")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned);
                break;
            }
        }

        let trace_id = trace_id.expect("shutdown lifecycle trace should include trace_id");
        let trace_ref = trace_ref.expect("shutdown lifecycle trace should include trace_ref");
        assert!(
            trace_id.starts_with("sha256:"),
            "redaction should hash sensitive trace_id field by default"
        );
        assert!(
            trace_ref.starts_with("ref-"),
            "trace_ref should remain available for correlation"
        );
    }
