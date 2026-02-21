use super::*;

impl<B: SdkBackend> LxmfSdkTopics for Client<B> {
    fn topic_create(
        &self,
        req: crate::domain::TopicCreateRequest,
    ) -> Result<crate::domain::TopicRecord, SdkError> {
        self.backend.topic_create(req)
    }

    fn topic_get(
        &self,
        topic_id: crate::domain::TopicId,
    ) -> Result<Option<crate::domain::TopicRecord>, SdkError> {
        self.backend.topic_get(topic_id)
    }

    fn topic_list(
        &self,
        req: crate::domain::TopicListRequest,
    ) -> Result<crate::domain::TopicListResult, SdkError> {
        self.backend.topic_list(req)
    }

    fn topic_subscribe(
        &self,
        req: crate::domain::TopicSubscriptionRequest,
    ) -> Result<Ack, SdkError> {
        self.backend.topic_subscribe(req)
    }

    fn topic_unsubscribe(&self, topic_id: crate::domain::TopicId) -> Result<Ack, SdkError> {
        self.backend.topic_unsubscribe(topic_id)
    }

    fn topic_publish(&self, req: crate::domain::TopicPublishRequest) -> Result<Ack, SdkError> {
        self.backend.topic_publish(req)
    }
}

impl<B: SdkBackend> LxmfSdkTelemetry for Client<B> {
    fn telemetry_query(
        &self,
        query: crate::domain::TelemetryQuery,
    ) -> Result<Vec<crate::domain::TelemetryPoint>, SdkError> {
        self.backend.telemetry_query(query)
    }

    fn telemetry_subscribe(&self, query: crate::domain::TelemetryQuery) -> Result<Ack, SdkError> {
        self.backend.telemetry_subscribe(query)
    }
}

impl<B: SdkBackend> LxmfSdkAttachments for Client<B> {
    fn attachment_store(
        &self,
        req: crate::domain::AttachmentStoreRequest,
    ) -> Result<crate::domain::AttachmentMeta, SdkError> {
        self.backend.attachment_store(req)
    }

    fn attachment_get(
        &self,
        attachment_id: crate::domain::AttachmentId,
    ) -> Result<Option<crate::domain::AttachmentMeta>, SdkError> {
        self.backend.attachment_get(attachment_id)
    }

    fn attachment_list(
        &self,
        req: crate::domain::AttachmentListRequest,
    ) -> Result<crate::domain::AttachmentListResult, SdkError> {
        self.backend.attachment_list(req)
    }

    fn attachment_delete(
        &self,
        attachment_id: crate::domain::AttachmentId,
    ) -> Result<Ack, SdkError> {
        self.backend.attachment_delete(attachment_id)
    }

    fn attachment_download(
        &self,
        attachment_id: crate::domain::AttachmentId,
    ) -> Result<Ack, SdkError> {
        self.backend.attachment_download(attachment_id)
    }

    fn attachment_upload_start(
        &self,
        req: crate::domain::AttachmentUploadStartRequest,
    ) -> Result<crate::domain::AttachmentUploadSession, SdkError> {
        self.backend.attachment_upload_start(req)
    }

    fn attachment_upload_chunk(
        &self,
        req: crate::domain::AttachmentUploadChunkRequest,
    ) -> Result<crate::domain::AttachmentUploadChunkAck, SdkError> {
        self.backend.attachment_upload_chunk(req)
    }

    fn attachment_upload_commit(
        &self,
        req: crate::domain::AttachmentUploadCommitRequest,
    ) -> Result<crate::domain::AttachmentMeta, SdkError> {
        self.backend.attachment_upload_commit(req)
    }

    fn attachment_download_chunk(
        &self,
        req: crate::domain::AttachmentDownloadChunkRequest,
    ) -> Result<crate::domain::AttachmentDownloadChunk, SdkError> {
        self.backend.attachment_download_chunk(req)
    }

    fn attachment_associate_topic(
        &self,
        attachment_id: crate::domain::AttachmentId,
        topic_id: crate::domain::TopicId,
    ) -> Result<Ack, SdkError> {
        self.backend.attachment_associate_topic(attachment_id, topic_id)
    }
}

impl<B: SdkBackend> LxmfSdkMarkers for Client<B> {
    fn marker_create(
        &self,
        req: crate::domain::MarkerCreateRequest,
    ) -> Result<crate::domain::MarkerRecord, SdkError> {
        self.backend.marker_create(req)
    }

    fn marker_list(
        &self,
        req: crate::domain::MarkerListRequest,
    ) -> Result<crate::domain::MarkerListResult, SdkError> {
        self.backend.marker_list(req)
    }

    fn marker_update_position(
        &self,
        req: crate::domain::MarkerUpdatePositionRequest,
    ) -> Result<crate::domain::MarkerRecord, SdkError> {
        self.backend.marker_update_position(req)
    }

    fn marker_delete(&self, req: crate::domain::MarkerDeleteRequest) -> Result<Ack, SdkError> {
        self.backend.marker_delete(req)
    }
}

impl<B: SdkBackend> LxmfSdkIdentity for Client<B> {
    fn identity_list(&self) -> Result<Vec<crate::domain::IdentityBundle>, SdkError> {
        self.backend.identity_list()
    }

    fn identity_announce_now(&self) -> Result<Ack, SdkError> {
        self.backend.identity_announce_now()
    }

    fn identity_presence_list(
        &self,
        req: crate::domain::PresenceListRequest,
    ) -> Result<crate::domain::PresenceListResult, SdkError> {
        self.backend.identity_presence_list(req)
    }

    fn identity_activate(&self, identity: crate::domain::IdentityRef) -> Result<Ack, SdkError> {
        self.backend.identity_activate(identity)
    }

    fn identity_import(
        &self,
        req: crate::domain::IdentityImportRequest,
    ) -> Result<crate::domain::IdentityBundle, SdkError> {
        self.backend.identity_import(req)
    }

    fn identity_export(
        &self,
        identity: crate::domain::IdentityRef,
    ) -> Result<crate::domain::IdentityImportRequest, SdkError> {
        self.backend.identity_export(identity)
    }

    fn identity_resolve(
        &self,
        req: crate::domain::IdentityResolveRequest,
    ) -> Result<Option<crate::domain::IdentityRef>, SdkError> {
        self.backend.identity_resolve(req)
    }

    fn identity_contact_update(
        &self,
        req: crate::domain::ContactUpdateRequest,
    ) -> Result<crate::domain::ContactRecord, SdkError> {
        self.backend.identity_contact_update(req)
    }

    fn identity_contact_list(
        &self,
        req: crate::domain::ContactListRequest,
    ) -> Result<crate::domain::ContactListResult, SdkError> {
        self.backend.identity_contact_list(req)
    }

    fn identity_bootstrap(
        &self,
        req: crate::domain::IdentityBootstrapRequest,
    ) -> Result<crate::domain::ContactRecord, SdkError> {
        self.backend.identity_bootstrap(req)
    }
}

impl<B: SdkBackend> LxmfSdkPaper for Client<B> {
    fn paper_encode(
        &self,
        message_id: MessageId,
    ) -> Result<crate::domain::PaperMessageEnvelope, SdkError> {
        self.backend.paper_encode(message_id)
    }

    fn paper_decode(&self, envelope: crate::domain::PaperMessageEnvelope) -> Result<Ack, SdkError> {
        self.backend.paper_decode(envelope)
    }
}

impl<B: SdkBackend> LxmfSdkRemoteCommands for Client<B> {
    fn command_invoke(
        &self,
        req: crate::domain::RemoteCommandRequest,
    ) -> Result<crate::domain::RemoteCommandResponse, SdkError> {
        self.backend.command_invoke(req)
    }

    fn command_reply(
        &self,
        correlation_id: String,
        reply: crate::domain::RemoteCommandResponse,
    ) -> Result<Ack, SdkError> {
        self.backend.command_reply(correlation_id, reply)
    }
}

impl<B: SdkBackend> LxmfSdkVoiceSignaling for Client<B> {
    fn voice_session_open(
        &self,
        req: crate::domain::VoiceSessionOpenRequest,
    ) -> Result<crate::domain::VoiceSessionId, SdkError> {
        self.backend.voice_session_open(req)
    }

    fn voice_session_update(
        &self,
        req: crate::domain::VoiceSessionUpdateRequest,
    ) -> Result<crate::domain::VoiceSessionState, SdkError> {
        self.backend.voice_session_update(req)
    }

    fn voice_session_close(
        &self,
        session_id: crate::domain::VoiceSessionId,
    ) -> Result<Ack, SdkError> {
        self.backend.voice_session_close(session_id)
    }
}
