use crate::capability::{NegotiationRequest, NegotiationResponse};
use crate::domain::{
    AttachmentDownloadChunk, AttachmentDownloadChunkRequest, AttachmentId, AttachmentListRequest,
    AttachmentListResult, AttachmentMeta, AttachmentStoreRequest, AttachmentUploadChunkAck,
    AttachmentUploadChunkRequest, AttachmentUploadCommitRequest, AttachmentUploadSession,
    AttachmentUploadStartRequest, ContactListRequest, ContactListResult, ContactRecord,
    ContactUpdateRequest, IdentityBootstrapRequest, IdentityBundle, IdentityImportRequest,
    IdentityRef, IdentityResolveRequest, MarkerCreateRequest, MarkerDeleteRequest,
    MarkerListRequest, MarkerListResult, MarkerRecord, MarkerUpdatePositionRequest,
    PaperMessageEnvelope, PresenceListRequest, PresenceListResult, RemoteCommandRequest,
    RemoteCommandResponse, TelemetryPoint, TelemetryQuery, TopicCreateRequest, TopicId,
    TopicListRequest, TopicListResult, TopicPublishRequest, TopicRecord, TopicSubscriptionRequest,
    VoiceSessionId, VoiceSessionOpenRequest, VoiceSessionState, VoiceSessionUpdateRequest,
};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, MessageId, RuntimeSnapshot, SendRequest,
    ShutdownMode, TickBudget, TickResult,
};
use serde::{Deserialize, Serialize};

const CAP_KEY_MANAGEMENT: &str = "sdk.capability.key_management";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KeyProviderClass {
    InMemory,
    File,
    OsKeystore,
    Hsm,
    Custom(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SdkKeyPurpose {
    IdentitySigning,
    TransportDh,
    SharedSecret,
    Custom(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SdkStoredKey {
    pub key_id: String,
    pub purpose: SdkKeyPurpose,
    pub material: Vec<u8>,
}

pub trait SdkBackend: Send + Sync {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError>;

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError>;

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError>;

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError>;

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError>;

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError>;

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError>;

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError>;

    fn tick(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
        Err(SdkError::new(
            code::CAPABILITY_DISABLED,
            ErrorCategory::Capability,
            "backend does not support manual ticking",
        ))
    }

    fn topic_create(&self, _req: TopicCreateRequest) -> Result<TopicRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topics"))
    }

    fn topic_get(&self, _topic_id: TopicId) -> Result<Option<TopicRecord>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topics"))
    }

    fn topic_list(&self, _req: TopicListRequest) -> Result<TopicListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topics"))
    }

    fn topic_subscribe(&self, _req: TopicSubscriptionRequest) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topic_subscriptions"))
    }

    fn topic_unsubscribe(&self, _topic_id: TopicId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topic_subscriptions"))
    }

    fn topic_publish(&self, _req: TopicPublishRequest) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.topic_fanout"))
    }

    fn telemetry_query(&self, _query: TelemetryQuery) -> Result<Vec<TelemetryPoint>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.telemetry_query"))
    }

    fn telemetry_subscribe(&self, _query: TelemetryQuery) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.telemetry_stream"))
    }

    fn attachment_store(&self, _req: AttachmentStoreRequest) -> Result<AttachmentMeta, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_get(
        &self,
        _attachment_id: AttachmentId,
    ) -> Result<Option<AttachmentMeta>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_list(
        &self,
        _req: AttachmentListRequest,
    ) -> Result<AttachmentListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_delete(&self, _attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_delete"))
    }

    fn attachment_download(&self, _attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn attachment_upload_start(
        &self,
        _req: AttachmentUploadStartRequest,
    ) -> Result<AttachmentUploadSession, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_upload_chunk(
        &self,
        _req: AttachmentUploadChunkRequest,
    ) -> Result<AttachmentUploadChunkAck, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_upload_commit(
        &self,
        _req: AttachmentUploadCommitRequest,
    ) -> Result<AttachmentMeta, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_download_chunk(
        &self,
        _req: AttachmentDownloadChunkRequest,
    ) -> Result<AttachmentDownloadChunk, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachment_streaming"))
    }

    fn attachment_associate_topic(
        &self,
        _attachment_id: AttachmentId,
        _topic_id: TopicId,
    ) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.attachments"))
    }

    fn marker_create(&self, _req: MarkerCreateRequest) -> Result<MarkerRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn marker_list(&self, _req: MarkerListRequest) -> Result<MarkerListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn marker_update_position(
        &self,
        _req: MarkerUpdatePositionRequest,
    ) -> Result<MarkerRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn marker_delete(&self, _req: MarkerDeleteRequest) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }

    fn identity_list(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_multi"))
    }

    fn identity_announce_now(&self) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_discovery"))
    }

    fn identity_presence_list(
        &self,
        _req: PresenceListRequest,
    ) -> Result<PresenceListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_discovery"))
    }

    fn identity_activate(&self, _identity: IdentityRef) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_multi"))
    }

    fn identity_import(&self, _req: IdentityImportRequest) -> Result<IdentityBundle, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_import_export"))
    }

    fn identity_export(&self, _identity: IdentityRef) -> Result<IdentityImportRequest, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_import_export"))
    }

    fn identity_resolve(
        &self,
        _req: IdentityResolveRequest,
    ) -> Result<Option<IdentityRef>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_hash_resolution"))
    }

    fn identity_contact_update(
        &self,
        _req: ContactUpdateRequest,
    ) -> Result<ContactRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.contact_management"))
    }

    fn identity_contact_list(
        &self,
        _req: ContactListRequest,
    ) -> Result<ContactListResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.contact_management"))
    }

    fn identity_bootstrap(
        &self,
        _req: IdentityBootstrapRequest,
    ) -> Result<ContactRecord, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.contact_management"))
    }

    fn paper_encode(&self, _message_id: MessageId) -> Result<PaperMessageEnvelope, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }

    fn paper_decode(&self, _envelope: PaperMessageEnvelope) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }

    fn command_invoke(
        &self,
        _req: RemoteCommandRequest,
    ) -> Result<RemoteCommandResponse, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.remote_commands"))
    }

    fn command_reply(
        &self,
        _correlation_id: String,
        _reply: RemoteCommandResponse,
    ) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.remote_commands"))
    }

    fn voice_session_open(
        &self,
        _req: VoiceSessionOpenRequest,
    ) -> Result<VoiceSessionId, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.voice_signaling"))
    }

    fn voice_session_update(
        &self,
        _req: VoiceSessionUpdateRequest,
    ) -> Result<VoiceSessionState, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.voice_signaling"))
    }

    fn voice_session_close(&self, _session_id: VoiceSessionId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.voice_signaling"))
    }
}

pub trait SdkBackendKeyManagement: SdkBackend {
    fn key_provider_class(&self) -> Result<KeyProviderClass, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_get(&self, _key_id: &str) -> Result<Option<SdkStoredKey>, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_put(&self, _key: SdkStoredKey) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_delete(&self, _key_id: &str) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }

    fn key_list_ids(&self) -> Result<Vec<String>, SdkError> {
        Err(SdkError::capability_disabled(CAP_KEY_MANAGEMENT))
    }
}

#[cfg(feature = "sdk-async")]
pub trait SdkBackendAsyncEvents: SdkBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;
}

#[cfg(not(feature = "sdk-async"))]
pub trait SdkBackendAsyncEvents: SdkBackend {}

#[cfg(all(feature = "rpc-backend", feature = "std"))]
pub mod rpc;

#[cfg(test)]
mod tests {
    use super::{
        KeyProviderClass, SdkBackend, SdkBackendKeyManagement, SdkKeyPurpose, SdkStoredKey,
    };
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
}
