use super::*;
use std::collections::BTreeMap;

fn base_config() -> SdkConfig {
    SdkConfig {
        profile: Profile::DesktopFull,
        bind_mode: BindMode::LocalOnly,
        auth_mode: AuthMode::LocalTrusted,
        overflow_policy: OverflowPolicy::Reject,
        block_timeout_ms: None,
        event_stream: EventStreamConfig {
            max_poll_events: 128,
            max_event_bytes: 32_768,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
        },
        idempotency_ttl_ms: 86_400_000,
        redaction: RedactionConfig {
            enabled: true,
            sensitive_transform: RedactionTransform::Hash,
            break_glass_allowed: false,
            break_glass_ttl_ms: None,
        },
        rpc_backend: None,
        extensions: BTreeMap::new(),
    }
}

#[test]
fn start_request_rejects_duplicate_contract_versions() {
    let request = StartRequest {
        supported_contract_versions: vec![2, 2],
        requested_capabilities: vec!["sdk.capability.cursor_replay".to_owned()],
        config: base_config(),
    };
    let err = request.validate().expect_err("duplicate versions must fail");
    assert_eq!(err.machine_code, crate::error::code::VALIDATION_INVALID_ARGUMENT);
}

#[test]
fn config_rejects_remote_bind_without_token_or_mtls() {
    let mut config = base_config();
    config.bind_mode = BindMode::Remote;
    config.auth_mode = AuthMode::LocalTrusted;
    let err = config.validate().expect_err("remote bind requires explicit secure auth");
    assert_eq!(err.machine_code, crate::error::code::SECURITY_REMOTE_BIND_DISALLOWED);
}

#[test]
fn config_accepts_local_trusted_profile() {
    let config = base_config();
    assert!(config.validate().is_ok());
}

#[test]
fn config_rejects_embedded_profile_with_mtls_auth_mode() {
    let mut config = base_config();
    config.profile = Profile::EmbeddedAlloc;
    config.bind_mode = BindMode::Remote;
    config.auth_mode = AuthMode::Mtls;
    config.rpc_backend = Some(RpcBackendConfig {
        listen_addr: "127.0.0.1:4243".to_string(),
        read_timeout_ms: 1_000,
        write_timeout_ms: 1_000,
        max_header_bytes: 8_192,
        max_body_bytes: 65_536,
        token_auth: None,
        mtls_auth: Some(MtlsAuthConfig {
            ca_bundle_path: "/tmp/ca.pem".to_string(),
            require_client_cert: true,
            allowed_san: Some("urn:test-san".to_string()),
            client_cert_path: Some("/tmp/client.pem".to_string()),
            client_key_path: Some("/tmp/client.key".to_string()),
        }),
    });
    let err = config.validate().expect_err("embedded profile must reject mtls");
    assert_eq!(err.machine_code, crate::error::code::VALIDATION_INVALID_ARGUMENT);
}

#[test]
fn config_rejects_mtls_without_client_cert_material_when_required() {
    let mut config = base_config();
    config.bind_mode = BindMode::Remote;
    config.auth_mode = AuthMode::Mtls;
    config.rpc_backend = Some(RpcBackendConfig {
        listen_addr: "127.0.0.1:4243".to_string(),
        read_timeout_ms: 1_000,
        write_timeout_ms: 1_000,
        max_header_bytes: 8_192,
        max_body_bytes: 65_536,
        token_auth: None,
        mtls_auth: Some(MtlsAuthConfig {
            ca_bundle_path: "/tmp/ca.pem".to_string(),
            require_client_cert: true,
            allowed_san: Some("urn:test-san".to_string()),
            client_cert_path: None,
            client_key_path: None,
        }),
    });
    let err = config.validate().expect_err("required mtls client cert paths must be provided");
    assert_eq!(err.machine_code, crate::error::code::SECURITY_AUTH_REQUIRED);
}

#[test]
fn config_patch_serialization_preserves_absent_vs_null() {
    let absent_patch = ConfigPatch {
        overflow_policy: None,
        block_timeout_ms: None,
        event_stream: None,
        idempotency_ttl_ms: None,
        redaction: None,
        rpc_backend: None,
        extensions: None,
    };
    let absent_json = serde_json::to_value(&absent_patch).expect("serialize absent patch");
    assert!(!absent_json.as_object().expect("object").contains_key("overflow_policy"));

    let clear_patch = ConfigPatch {
        overflow_policy: Some(None),
        block_timeout_ms: None,
        event_stream: None,
        idempotency_ttl_ms: None,
        redaction: None,
        rpc_backend: None,
        extensions: None,
    };
    let clear_json = serde_json::to_value(&clear_patch).expect("serialize clear patch");
    assert!(clear_json["overflow_policy"].is_null());
}

#[test]
fn delivery_state_deserializes_unknown_variant() {
    let value = serde_json::json!("future_state");
    let state: DeliveryState =
        serde_json::from_value(value).expect("unknown delivery state should map to Unknown");
    assert_eq!(state, DeliveryState::Unknown);
}

#[test]
fn runtime_state_deserializes_unknown_variant() {
    let value = serde_json::json!("maintenance");
    let state: RuntimeState =
        serde_json::from_value(value).expect("unknown runtime state should map to Unknown");
    assert_eq!(state, RuntimeState::Unknown);
}

#[test]
fn start_request_builder_defaults_and_customization_validate() {
    let request = StartRequest::new(SdkConfig::desktop_full_default())
        .with_requested_capability("sdk.capability.cursor_replay")
        .with_supported_contract_versions(vec![2, 1]);
    assert_eq!(request.supported_contract_versions, vec![2, 1]);
    assert_eq!(request.requested_capabilities, vec!["sdk.capability.cursor_replay"]);
    assert!(request.validate().is_ok());
}

#[test]
fn send_request_builder_sets_optional_fields_and_extensions() {
    let request = SendRequest::new(
        "source",
        "destination",
        serde_json::json!({"title": "hello", "content": "world"}),
    )
    .with_idempotency_key("idem-1")
    .with_ttl_ms(42_000)
    .with_correlation_id("corr-1")
    .with_extension("sdk.ext.example", serde_json::json!({"enabled": true}));

    assert_eq!(request.source, "source");
    assert_eq!(request.destination, "destination");
    assert_eq!(request.idempotency_key.as_deref(), Some("idem-1"));
    assert_eq!(request.ttl_ms, Some(42_000));
    assert_eq!(request.correlation_id.as_deref(), Some("corr-1"));
    assert_eq!(request.extensions.len(), 1);
}

#[test]
fn sdk_config_default_profiles_validate() {
    assert!(SdkConfig::desktop_local_default().validate().is_ok());
    assert!(SdkConfig::desktop_full_default().validate().is_ok());
    assert!(SdkConfig::embedded_alloc_default().validate().is_ok());
}

#[test]
fn sdk_config_remote_auth_helpers_apply_valid_security_modes() {
    let token = SdkConfig::desktop_full_default().with_token_auth("issuer", "audience", "secret");
    assert!(token.validate().is_ok());
    assert_eq!(token.bind_mode, BindMode::Remote);
    assert_eq!(token.auth_mode, AuthMode::Token);

    let mtls = SdkConfig::desktop_full_default()
        .with_mtls_auth("/tmp/ca.pem")
        .with_mtls_client_credentials("/tmp/client.pem", "/tmp/client.key");
    assert!(mtls.validate().is_ok());
    assert_eq!(mtls.bind_mode, BindMode::Remote);
    assert_eq!(mtls.auth_mode, AuthMode::Mtls);
}

#[test]
fn config_patch_builder_accumulates_mutations() {
    let patch = ConfigPatch::new()
        .with_overflow_policy(OverflowPolicy::Block)
        .with_block_timeout_ms(250)
        .with_idempotency_ttl_ms(5_000)
        .with_extension("sdk.ext.sample", serde_json::json!("on"));
    assert!(!patch.is_empty());
    assert_eq!(patch.block_timeout_ms, Some(Some(250)));
    assert_eq!(patch.idempotency_ttl_ms, Some(Some(5_000)));
    assert!(patch.extensions.as_ref().and_then(Option::as_ref).is_some());
}
