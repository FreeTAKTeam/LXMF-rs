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
