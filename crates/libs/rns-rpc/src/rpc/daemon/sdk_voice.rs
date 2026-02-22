impl RpcDaemon {
    fn handle_sdk_voice_session_open_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.voice_signaling") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_voice_session_open_v2",
                "sdk.capability.voice_signaling",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkVoiceSessionOpenV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let peer_id = match Self::normalize_non_empty(parsed.peer_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "peer_id must not be empty",
                ))
            }
        };
        let session_id = self.next_sdk_domain_id("voice");
        let record = SdkVoiceSessionRecord {
            session_id: session_id.clone(),
            peer_id,
            codec_hint: parsed.codec_hint,
            state: "ringing".to_string(),
            extensions: parsed.extensions,
        };
        self.sdk_voice_sessions
            .lock()
            .expect("sdk_voice_sessions mutex poisoned")
            .insert(session_id.clone(), record);
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "session_id": session_id })),
            error: None,
        })
    }

    fn handle_sdk_voice_session_update_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.voice_signaling") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_voice_session_update_v2",
                "sdk.capability.voice_signaling",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkVoiceSessionUpdateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let session_id = match Self::normalize_non_empty(parsed.session_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "session_id must not be empty",
                ))
            }
        };
        let Some(next_state) = Self::normalize_voice_state(parsed.state.as_str()) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "voice state is invalid",
            ));
        };
        {
            let mut sessions =
                self.sdk_voice_sessions.lock().expect("sdk_voice_sessions mutex poisoned");
            let Some(session) = sessions.get_mut(session_id.as_str()) else {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_NOT_FOUND",
                    "voice session not found",
                ));
            };
            let current_state = session.state.clone();
            let current_rank = Self::voice_state_rank(current_state.as_str());
            let next_rank = Self::voice_state_rank(next_state);
            if current_rank == 4 && current_state != next_state {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "voice session is already terminal",
                ));
            }
            if next_rank < current_rank && next_rank != 4 {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "voice session transitions must be monotonic",
                ));
            }
            session.state = next_state.to_string();
            session.extensions = parsed.extensions;
        }
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "state": next_state })),
            error: None,
        })
    }

    fn handle_sdk_voice_session_close_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.voice_signaling") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_voice_session_close_v2",
                "sdk.capability.voice_signaling",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkVoiceSessionCloseV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let session_id = match Self::normalize_non_empty(parsed.session_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "session_id must not be empty",
                ))
            }
        };
        {
            let mut sessions =
                self.sdk_voice_sessions.lock().expect("sdk_voice_sessions mutex poisoned");
            let Some(session) = sessions.get_mut(session_id.as_str()) else {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_NOT_FOUND",
                    "voice session not found",
                ));
            };
            session.state = "closed".to_string();
        }
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "session_id": session_id })),
            error: None,
        })
    }

}
