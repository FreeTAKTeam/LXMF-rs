use super::*;
use serde_json::json;

impl RpcBackendClient {
    pub(super) fn topic_create_impl(
        &self,
        req: TopicCreateRequest,
    ) -> Result<TopicRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_create_v2", Some(params))?;
        Self::decode_field_or_root(&result, "topic", "topic_create response")
    }

    pub(super) fn topic_get_impl(
        &self,
        topic_id: TopicId,
    ) -> Result<Option<TopicRecord>, SdkError> {
        let result = self.call_rpc(
            "sdk_topic_get_v2",
            Some(json!({
                "topic_id": topic_id.0,
            })),
        )?;
        if result.get("topic").is_some() {
            return Self::decode_optional_field(&result, "topic", "topic_get response");
        }
        if result.is_null() {
            return Ok(None);
        }
        Self::decode_value(result, "topic_get response").map(Some)
    }

    pub(super) fn topic_list_impl(
        &self,
        req: TopicListRequest,
    ) -> Result<TopicListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "topic_list", "topic_list response")
    }

    pub(super) fn topic_subscribe_impl(
        &self,
        req: TopicSubscriptionRequest,
    ) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_subscribe_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn topic_unsubscribe_impl(&self, topic_id: TopicId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_topic_unsubscribe_v2",
            Some(json!({
                "topic_id": topic_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn topic_publish_impl(&self, req: TopicPublishRequest) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_publish_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn telemetry_query_impl(
        &self,
        query: TelemetryQuery,
    ) -> Result<Vec<TelemetryPoint>, SdkError> {
        let params = serde_json::to_value(query).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_telemetry_query_v2", Some(params))?;
        if let Some(points) = result.get("points") {
            return Self::decode_value(points.clone(), "telemetry_query points");
        }
        Self::decode_value(result, "telemetry_query points")
    }

    pub(super) fn telemetry_subscribe_impl(&self, query: TelemetryQuery) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(query).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_telemetry_subscribe_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn attachment_store_impl(
        &self,
        req: AttachmentStoreRequest,
    ) -> Result<AttachmentMeta, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_store_v2", Some(params))?;
        Self::decode_field_or_root(&result, "attachment", "attachment_store response")
    }

    pub(super) fn attachment_get_impl(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<Option<AttachmentMeta>, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_get_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
            })),
        )?;
        if result.get("attachment").is_some() {
            return Self::decode_optional_field(&result, "attachment", "attachment_get response");
        }
        if result.is_null() {
            return Ok(None);
        }
        Self::decode_value(result, "attachment_get response").map(Some)
    }

    pub(super) fn attachment_list_impl(
        &self,
        req: AttachmentListRequest,
    ) -> Result<AttachmentListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "attachment_list", "attachment_list response")
    }

    pub(super) fn attachment_delete_impl(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_delete_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn attachment_download_impl(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_download_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn attachment_upload_start_impl(
        &self,
        req: AttachmentUploadStartRequest,
    ) -> Result<AttachmentUploadSession, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_upload_start_v2", Some(params))?;
        Self::decode_field_or_root(&result, "upload", "attachment_upload_start response")
    }

    pub(super) fn attachment_upload_chunk_impl(
        &self,
        req: AttachmentUploadChunkRequest,
    ) -> Result<AttachmentUploadChunkAck, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_upload_chunk_v2", Some(params))?;
        Self::decode_field_or_root(&result, "upload_chunk", "attachment_upload_chunk response")
    }

    pub(super) fn attachment_upload_commit_impl(
        &self,
        req: AttachmentUploadCommitRequest,
    ) -> Result<AttachmentMeta, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_upload_commit_v2", Some(params))?;
        Self::decode_field_or_root(&result, "attachment", "attachment_upload_commit response")
    }

    pub(super) fn attachment_download_chunk_impl(
        &self,
        req: AttachmentDownloadChunkRequest,
    ) -> Result<AttachmentDownloadChunk, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_download_chunk_v2", Some(params))?;
        Self::decode_field_or_root(&result, "download_chunk", "attachment_download_chunk response")
    }

    pub(super) fn attachment_associate_topic_impl(
        &self,
        attachment_id: AttachmentId,
        topic_id: TopicId,
    ) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_associate_topic_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
                "topic_id": topic_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn marker_create_impl(
        &self,
        req: MarkerCreateRequest,
    ) -> Result<MarkerRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_marker_create_v2", Some(params))?;
        Self::decode_field_or_root(&result, "marker", "marker_create response")
    }

    pub(super) fn marker_list_impl(
        &self,
        req: MarkerListRequest,
    ) -> Result<MarkerListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_marker_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "marker_list", "marker_list response")
    }

    pub(super) fn marker_update_position_impl(
        &self,
        req: MarkerUpdatePositionRequest,
    ) -> Result<MarkerRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_marker_update_position_v2", Some(params))?;
        Self::decode_field_or_root(&result, "marker", "marker_update_position response")
    }

    pub(super) fn marker_delete_impl(&self, marker_id: MarkerId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_marker_delete_v2",
            Some(json!({
                "marker_id": marker_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn identity_list_impl(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        let result = self.call_rpc("sdk_identity_list_v2", Some(json!({})))?;
        if let Some(identities) = result.get("identities") {
            return Self::decode_value(identities.clone(), "identity_list response");
        }
        Self::decode_value(result, "identity_list response")
    }

    pub(super) fn identity_activate_impl(&self, identity: IdentityRef) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_identity_activate_v2",
            Some(json!({
                "identity": identity.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn identity_import_impl(
        &self,
        req: IdentityImportRequest,
    ) -> Result<IdentityBundle, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_import_v2", Some(params))?;
        Self::decode_field_or_root(&result, "identity", "identity_import response")
    }

    pub(super) fn identity_export_impl(
        &self,
        identity: IdentityRef,
    ) -> Result<IdentityImportRequest, SdkError> {
        let result = self.call_rpc(
            "sdk_identity_export_v2",
            Some(json!({
                "identity": identity.0,
            })),
        )?;
        Self::decode_field_or_root(&result, "bundle", "identity_export response")
    }

    pub(super) fn identity_resolve_impl(
        &self,
        req: IdentityResolveRequest,
    ) -> Result<Option<IdentityRef>, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_resolve_v2", Some(params))?;
        if result.get("identity").is_some() {
            return Self::decode_optional_field(&result, "identity", "identity_resolve response");
        }
        if result.is_null() {
            return Ok(None);
        }
        Self::decode_value(result, "identity_resolve response").map(Some)
    }

    pub(super) fn paper_encode_impl(
        &self,
        message_id: MessageId,
    ) -> Result<PaperMessageEnvelope, SdkError> {
        let result = self.call_rpc(
            "sdk_paper_encode_v2",
            Some(json!({
                "message_id": message_id.0,
            })),
        )?;
        Self::decode_field_or_root(&result, "envelope", "paper_encode response")
    }

    pub(super) fn paper_decode_impl(
        &self,
        envelope: PaperMessageEnvelope,
    ) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_paper_decode_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn command_invoke_impl(
        &self,
        req: RemoteCommandRequest,
    ) -> Result<RemoteCommandResponse, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_command_invoke_v2", Some(params))?;
        Self::decode_field_or_root(&result, "response", "command_invoke response")
    }

    pub(super) fn command_reply_impl(
        &self,
        correlation_id: String,
        reply: RemoteCommandResponse,
    ) -> Result<Ack, SdkError> {
        let mut params = serde_json::to_value(reply).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        if let Some(object) = params.as_object_mut() {
            object.insert("correlation_id".to_owned(), JsonValue::String(correlation_id));
        } else {
            return Err(SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                "command_reply payload serialization did not produce an object",
            ));
        }
        let result = self.call_rpc("sdk_command_reply_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    pub(super) fn voice_session_open_impl(
        &self,
        req: VoiceSessionOpenRequest,
    ) -> Result<VoiceSessionId, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_voice_session_open_v2", Some(params))?;
        if let Some(session_id) = result.get("session_id").and_then(JsonValue::as_str) {
            return Ok(VoiceSessionId(session_id.to_owned()));
        }
        Self::decode_value(result, "voice_session_open response")
    }

    pub(super) fn voice_session_update_impl(
        &self,
        req: VoiceSessionUpdateRequest,
    ) -> Result<VoiceSessionState, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_voice_session_update_v2", Some(params))?;
        if let Some(state) = result.get("state") {
            return Self::decode_value(state.clone(), "voice_session_update response");
        }
        Self::decode_value(result, "voice_session_update response")
    }

    pub(super) fn voice_session_close_impl(
        &self,
        session_id: VoiceSessionId,
    ) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_voice_session_close_v2",
            Some(json!({
                "session_id": session_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }
}
