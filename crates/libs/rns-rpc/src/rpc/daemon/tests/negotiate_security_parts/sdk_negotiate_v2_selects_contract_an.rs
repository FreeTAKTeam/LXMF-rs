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
        assert_eq!(result["contract_release"], json!("v2.6"));
        assert_eq!(result["sdk_version"].as_str(), Some(expected_lxmf_sdk_version().as_str()));
        assert_eq!(
            result["python_reference"],
            json!({
                "reticulum_conformance_ref": expected_python_reference("RETICULUM_CONFORMANCE_REF"),
                "python_reticulum_version": crate::PYTHON_RETICULUM_REFERENCE_VERSION,
                "python_reticulum_ref": expected_python_reference("PYTHON_RETICULUM_REF"),
                "python_lxmf_version": crate::PYTHON_LXMF_REFERENCE_VERSION,
                "python_lxmf_ref": expected_python_reference("PYTHON_LXMF_REF"),
            })
        );
        assert_eq!(
            result["software_parity"],
            json!(crate::current_software_parity_orientation())
        );
        assert_eq!(
            result["meta"]["python_reference"],
            result["python_reference"],
            "response metadata should repeat the parity checkpoint for clients that only inspect meta"
        );
        assert_eq!(result["effective_limits"]["max_poll_events"], json!(64));
    }

    fn expected_lxmf_sdk_version() -> String {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("libs dir")
            .join("lxmf-sdk")
            .join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).expect("read lxmf-sdk Cargo.toml");
        text.lines()
            .find_map(|line| line.trim().strip_prefix("version = "))
            .map(|value| value.trim_matches('"').to_string())
            .expect("lxmf-sdk package version")
    }

    fn expected_python_reference(name: &str) -> String {
        let workflow = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repo root")
            .join(".github")
            .join("workflows")
            .join("python-interop.yml");
        let needle = format!("{name}: ");
        let text = std::fs::read_to_string(&workflow).expect("read python interop workflow");
        text.lines()
            .find_map(|line| line.trim().strip_prefix(&needle))
            .map(str::trim)
            .map(str::to_string)
            .expect("python reference env pin")
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
        let signature = RpcDaemon::token_signature("test-secret", payload.as_str());
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
        let expired_sig = RpcDaemon::token_signature("test-secret", expired_payload.as_str());
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
        assert_eq!(tampered.message, "token signature does not match runtime policy");

        let malformed_headers =
            vec![("authorization".to_string(), format!("Bearer {valid_payload};sig=not-hex"))];
        let malformed = daemon
            .authorize_http_request(&malformed_headers, Some("10.5.6.7"))
            .expect_err("malformed signature should be rejected");
        assert_eq!(malformed.code, "SDK_SECURITY_TOKEN_INVALID");
        assert_eq!(malformed.message, "token signature is invalid");
    }
