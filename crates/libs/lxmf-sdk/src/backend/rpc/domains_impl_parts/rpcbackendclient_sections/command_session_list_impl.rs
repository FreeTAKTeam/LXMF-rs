impl RpcBackendClient {

    pub(super) fn command_session_list_impl(
        &self,
        req: RemoteCommandSessionListRequest,
    ) -> Result<RemoteCommandSessionListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_command_session_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "session_list", "command_session_list response")
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
