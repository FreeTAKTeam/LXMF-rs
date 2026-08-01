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
    fn sdk_security_failed_authentication_is_rate_limited_before_token_parsing() {
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
        let configured = daemon
            .handle_rpc(rpc_request(
                26,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": {
                        "extensions": {
                            "rate_limits": {
                                "per_ip_per_minute": 2,
                                "per_principal_per_minute": 10
                            }
                        }
                    }
                }),
            ))
            .expect("configure rate limits");
        assert!(configured.error.is_none());

        for _ in 0..2 {
            let error = daemon
                .authorize_http_request(&[], Some("10.5.6.7"))
                .expect_err("missing credentials should fail");
            assert_eq!(error.code, "SDK_SECURITY_AUTH_REQUIRED");
        }
        let limited = daemon
            .authorize_http_request(&[], Some("10.5.6.7"))
            .expect_err("repeated missing credentials should be throttled");
        assert_eq!(limited.code, "SDK_SECURITY_RATE_LIMITED");
        assert!(
            daemon.sdk_rate_principal_counts
                .lock()
                .expect("sdk_rate_principal_counts mutex poisoned")
                .is_empty(),
            "failed authentication must not consume a principal quota"
        );
    }

    #[test]
    fn sdk_security_pre_authentication_ip_state_is_bounded() {
        let daemon = RpcDaemon::test_instance();

        for index in 0..super::sdk_auth_http::SDK_RATE_LIMIT_MAX_IP_ENTRIES + 128 {
            let source_ip = format!("198.51.100.{index}");
            let _ = daemon.enforce_pre_auth_ip_rate_limit(source_ip.as_str());
        }
        let oversized_source =
            "x".repeat(super::sdk_auth_http::SDK_RATE_LIMIT_MAX_IP_KEY_BYTES + 1);
        let _ = daemon.enforce_pre_auth_ip_rate_limit(oversized_source.as_str());

        let counts =
            daemon.sdk_rate_ip_counts.lock().expect("sdk_rate_ip_counts mutex poisoned");
        assert!(counts.len() <= super::sdk_auth_http::SDK_RATE_LIMIT_MAX_IP_ENTRIES);
        assert!(counts.contains_key(super::sdk_auth_http::SDK_RATE_LIMIT_OVERFLOW_IP));
        assert!(
            counts
                .keys()
                .all(|key| key.len() <= super::sdk_auth_http::SDK_RATE_LIMIT_MAX_IP_KEY_BYTES)
        );
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
