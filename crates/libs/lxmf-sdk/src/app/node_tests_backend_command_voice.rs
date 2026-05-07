macro_rules! mock_backend_command_voice_methods {
    () => {
    fn command_invoke(
        &self,
        req: crate::domain::RemoteCommandRequest,
    ) -> Result<crate::domain::RemoteCommandResponse, SdkError> {
        self.remote_command_results
            .lock()
            .expect("remote command results")
            .pop_front()
            .unwrap_or_else(|| {
                Ok(crate::domain::RemoteCommandResponse {
                    accepted: true,
                    payload: serde_json::json!({
                        "command": req.command,
                        "target": req.target,
                        "payload": req.payload,
                    }),
                    extensions: req.extensions,
                })
            })
    }

    fn envelope_execute(
        &self,
        envelope: crate::app::Envelope,
    ) -> Result<crate::app::EnvelopeResponse, SdkError> {
        self.envelope_results.lock().expect("envelope results").pop_front().unwrap_or_else(|| {
            Ok(crate::app::EnvelopeResponse {
                operation_id: envelope.operation_id,
                kind: crate::app::EnvelopeKind::Result,
                accepted: true,
                correlation_id: envelope.correlation_id,
                payload: serde_json::json!({
                    "query": true,
                    "payload": envelope.payload,
                }),
                extensions: envelope.extensions,
            })
        })
    }

    fn voice_session_open(
        &self,
        _req: crate::domain::VoiceSessionOpenRequest,
    ) -> Result<crate::domain::VoiceSessionId, SdkError> {
        self.voice_open_results
            .lock()
            .expect("voice open results")
            .pop_front()
            .unwrap_or_else(|| Ok(crate::domain::VoiceSessionId("voice-1".to_owned())))
    }

    fn voice_session_update(
        &self,
        _req: crate::domain::VoiceSessionUpdateRequest,
    ) -> Result<crate::domain::VoiceSessionState, SdkError> {
        self.voice_update_results
            .lock()
            .expect("voice update results")
            .pop_front()
            .unwrap_or(Ok(crate::domain::VoiceSessionState::Active))
    }

    fn voice_session_close(
        &self,
        _session_id: crate::domain::VoiceSessionId,
    ) -> Result<Ack, SdkError> {
        self.voice_close_results
            .lock()
            .expect("voice close results")
            .pop_front()
            .unwrap_or(Ok(Ack { accepted: true, revision: None }))
    }
    };
}
