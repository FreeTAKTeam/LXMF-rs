use super::{KeyProviderClass, SdkBackend, SdkBackendKeyManagement, SdkKeyPurpose, SdkStoredKey};
use crate::capability::{EffectiveLimits, NegotiationRequest, NegotiationResponse};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, MessageId, RuntimeSnapshot, RuntimeState,
    SendRequest, ShutdownMode, TickBudget,
};
use std::collections::BTreeMap;

struct NoKeyBackend;

impl SdkBackend for NoKeyBackend {
    fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        Ok(NegotiationResponse {
            runtime_id: "test-runtime".to_owned(),
            active_contract_version: 2,
            effective_capabilities: vec![],
            effective_limits: EffectiveLimits {
                max_poll_events: 16,
                max_event_bytes: 4096,
                max_batch_bytes: 65_536,
                max_extension_keys: 8,
                idempotency_ttl_ms: 1_000,
            },
            contract_release: "v2.5".to_owned(),
            schema_namespace: "v2".to_owned(),
        })
    }

    fn send(&self, _req: SendRequest) -> Result<MessageId, SdkError> {
        Ok(MessageId("msg-test".to_owned()))
    }

    fn cancel(&self, _id: MessageId) -> Result<CancelResult, SdkError> {
        Ok(CancelResult::NotFound)
    }

    fn status(&self, _id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        Ok(None)
    }

    fn configure(&self, _expected_revision: u64, _patch: ConfigPatch) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: Some(1) })
    }

    fn poll_events(
        &self,
        _cursor: Option<crate::event::EventCursor>,
        _max: usize,
    ) -> Result<EventBatch, SdkError> {
        Ok(EventBatch {
            events: Vec::new(),
            next_cursor: EventCursor("cursor-0".to_owned()),
            dropped_count: 0,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        })
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        Ok(RuntimeSnapshot {
            runtime_id: "test-runtime".to_owned(),
            state: RuntimeState::Running,
            active_contract_version: 2,
            event_stream_position: 0,
            config_revision: 1,
            queued_messages: 0,
            in_flight_messages: 0,
        })
    }

    fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: Some(2) })
    }

    fn tick(&self, _budget: TickBudget) -> Result<crate::types::TickResult, SdkError> {
        Err(SdkError::new(
            code::CAPABILITY_DISABLED,
            ErrorCategory::Capability,
            "manual ticking disabled",
        ))
    }
}

impl SdkBackendKeyManagement for NoKeyBackend {}

#[test]
fn sdk_backend_key_management_defaults_to_capability_disabled() {
    let backend = NoKeyBackend;
    for result in [
        backend.key_provider_class().map(|_| ()),
        backend.key_get("key-a").map(|_| ()),
        backend
            .key_put(SdkStoredKey {
                key_id: "key-a".to_owned(),
                purpose: SdkKeyPurpose::IdentitySigning,
                material: vec![1, 2, 3, 4],
            })
            .map(|_| ()),
        backend.key_delete("key-a").map(|_| ()),
        backend.key_list_ids().map(|_| ()),
    ] {
        let err = result.expect_err("key management methods should be disabled by default");
        assert_eq!(err.code(), code::CAPABILITY_DISABLED);
        assert_eq!(err.category, ErrorCategory::Capability);
        assert_eq!(
            err.details.get("capability_id").and_then(serde_json::Value::as_str),
            Some("sdk.capability.key_management")
        );
    }
}

#[test]
fn sdk_backend_key_management_types_roundtrip() {
    let value = SdkStoredKey {
        key_id: "hsm-identity".to_owned(),
        purpose: SdkKeyPurpose::IdentitySigning,
        material: vec![42, 7, 9],
    };
    let json = serde_json::to_value(&value).expect("serialize key");
    let parsed: SdkStoredKey = serde_json::from_value(json).expect("deserialize key");
    assert_eq!(parsed.key_id, "hsm-identity");

    let provider = KeyProviderClass::OsKeystore;
    let provider_json = serde_json::to_string(&provider).expect("serialize provider");
    assert_eq!(provider_json, "\"os_keystore\"");
}
