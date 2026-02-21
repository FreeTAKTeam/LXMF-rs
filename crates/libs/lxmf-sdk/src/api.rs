use crate::domain::{
    AttachmentDownloadChunk, AttachmentDownloadChunkRequest, AttachmentId, AttachmentListRequest,
    AttachmentListResult, AttachmentMeta, AttachmentStoreRequest, AttachmentUploadChunkAck,
    AttachmentUploadChunkRequest, AttachmentUploadCommitRequest, AttachmentUploadSession,
    AttachmentUploadStartRequest, IdentityBundle, IdentityImportRequest, IdentityRef,
    IdentityResolveRequest, MarkerCreateRequest, MarkerId, MarkerListRequest, MarkerListResult,
    MarkerRecord, MarkerUpdatePositionRequest, PaperMessageEnvelope, RemoteCommandRequest,
    RemoteCommandResponse, TelemetryPoint, TelemetryQuery, TopicCreateRequest, TopicId,
    TopicListRequest, TopicListResult, TopicPublishRequest, TopicRecord, TopicSubscriptionRequest,
    VoiceSessionId, VoiceSessionOpenRequest, VoiceSessionState, VoiceSessionUpdateRequest,
};
use crate::error::SdkError;
use crate::event::{EventBatch, EventCursor};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, CancelResult, ClientHandle, ConfigPatch, DeliverySnapshot, GroupSendRequest,
    GroupSendResult, MessageId, RuntimeSnapshot, SendRequest, ShutdownMode, StartRequest,
    TickBudget, TickResult,
};

pub trait LxmfSdk {
    fn start(&self, req: StartRequest) -> Result<ClientHandle, SdkError>;
    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError>;
    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError>;
    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError>;
    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError>;
    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError>;
    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError>;
    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError>;
}

pub trait LxmfSdkManualTick {
    fn tick(&self, budget: TickBudget) -> Result<TickResult, SdkError>;
}

#[cfg(feature = "sdk-async")]
pub trait LxmfSdkAsync {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError>;
}

#[cfg(not(feature = "sdk-async"))]
pub trait LxmfSdkAsync {}

pub trait LxmfSdkTopics {
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
}

pub trait LxmfSdkTelemetry {
    fn telemetry_query(&self, _query: TelemetryQuery) -> Result<Vec<TelemetryPoint>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.telemetry_query"))
    }

    fn telemetry_subscribe(&self, _query: TelemetryQuery) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.telemetry_stream"))
    }
}

pub trait LxmfSdkAttachments {
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
}

pub trait LxmfSdkMarkers {
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

    fn marker_delete(&self, _marker_id: MarkerId) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.markers"))
    }
}

pub trait LxmfSdkIdentity {
    fn identity_list(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.identity_multi"))
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
}

pub trait LxmfSdkPaper {
    fn paper_encode(&self, _message_id: MessageId) -> Result<PaperMessageEnvelope, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }

    fn paper_decode(&self, _envelope: PaperMessageEnvelope) -> Result<Ack, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.paper_messages"))
    }
}

pub trait LxmfSdkRemoteCommands {
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
}

pub trait LxmfSdkVoiceSignaling {
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

pub trait LxmfSdkGroupDelivery {
    fn send_group(&self, _req: GroupSendRequest) -> Result<GroupSendResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.group_delivery"))
    }
}
