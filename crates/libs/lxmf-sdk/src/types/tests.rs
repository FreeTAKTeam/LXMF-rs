use super::*;
use std::collections::BTreeMap;

fn base_config() -> SdkConfig {
    SdkConfig {
        profile: Profile::DesktopFull,
        bind_mode: BindMode::LocalOnly,
        auth_mode: AuthMode::LocalTrusted,
        overflow_policy: OverflowPolicy::Reject,
        block_timeout_ms: None,
        store_forward: StoreForwardConfig {
            max_messages: 50_000,
            max_message_age_ms: 604_800_000,
            capacity_policy: StoreForwardCapacityPolicy::DropOldest,
            eviction_priority: StoreForwardEvictionPriority::TerminalFirst,
        },
        event_stream: EventStreamConfig {
            max_poll_events: 128,
            max_event_bytes: 32_768,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
        },
        event_sink: EventSinkConfig {
            enabled: false,
            max_event_bytes: 65_536,
            allow_kinds: vec![EventSinkKind::Webhook, EventSinkKind::Mqtt, EventSinkKind::Custom],
            extensions: BTreeMap::new(),
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
fn config_rejects_zero_store_forward_limits() {
    let mut config = base_config();
    config.store_forward.max_messages = 0;
    let err =
        config.validate().expect_err("zero store-forward message capacity should fail validation");
    assert_eq!(err.machine_code, crate::error::code::VALIDATION_INVALID_ARGUMENT);

    let mut config = base_config();
    config.store_forward.max_messages = 1;
    config.store_forward.max_message_age_ms = 0;
    let err = config.validate().expect_err("zero store-forward age limit should fail validation");
    assert_eq!(err.machine_code, crate::error::code::VALIDATION_INVALID_ARGUMENT);
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
        store_forward: None,
        event_sink: None,
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
        store_forward: None,
        event_sink: None,
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
fn sdk_config_store_forward_helpers_apply_policy_mutations() {
    let config = SdkConfig::desktop_full_default()
        .with_store_forward_limits(4096, 120_000)
        .with_store_forward_policy(
            StoreForwardCapacityPolicy::RejectNew,
            StoreForwardEvictionPriority::OldestFirst,
        );
    assert_eq!(config.store_forward.max_messages, 4096);
    assert_eq!(config.store_forward.max_message_age_ms, 120_000);
    assert_eq!(config.store_forward.capacity_policy, StoreForwardCapacityPolicy::RejectNew);
    assert_eq!(config.store_forward.eviction_priority, StoreForwardEvictionPriority::OldestFirst);
}

#[test]
fn sdk_config_event_sink_helpers_apply_policy_mutations() {
    let config = SdkConfig::desktop_full_default().with_event_sink(
        true,
        4_096,
        vec![EventSinkKind::Webhook, EventSinkKind::Custom],
    );
    assert!(config.event_sink.enabled);
    assert_eq!(config.event_sink.max_event_bytes, 4_096);
    assert_eq!(config.event_sink.allow_kinds, vec![EventSinkKind::Webhook, EventSinkKind::Custom]);
}

#[test]
fn sdk_config_rejects_invalid_event_sink_policy() {
    let mut config = SdkConfig::desktop_full_default();
    config.event_sink.max_event_bytes = 128;
    let err = config.validate().expect_err("event sink max bytes below bound should fail");
    assert_eq!(err.machine_code, crate::error::code::VALIDATION_INVALID_ARGUMENT);

    let mut config = SdkConfig::desktop_full_default();
    config.event_sink.allow_kinds.clear();
    let err = config.validate().expect_err("event sink kinds cannot be empty");
    assert_eq!(err.machine_code, crate::error::code::VALIDATION_INVALID_ARGUMENT);

    let mut config = SdkConfig::desktop_full_default();
    config.event_sink.enabled = true;
    config.redaction.enabled = false;
    let err = config.validate().expect_err("event sink must enforce redaction");
    assert_eq!(err.machine_code, crate::error::code::SECURITY_REDACTION_REQUIRED);
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
