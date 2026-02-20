impl RpcDaemon {
    fn handle_sdk_identity_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_multi") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_list_v2",
                "sdk.capability.identity_multi",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkIdentityListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let mut identities = self
            .sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        identities.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "identities": identities })),
            error: None,
        })
    }

    fn handle_sdk_identity_activate_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_multi") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_activate_v2",
                "sdk.capability.identity_multi",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityActivateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let identity = match Self::normalize_non_empty(parsed.identity.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "identity must not be empty",
                ))
            }
        };
        if !self
            .sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .contains_key(identity.as_str())
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "identity not found",
            ));
        }
        *self.sdk_active_identity.lock().expect("sdk_active_identity mutex poisoned") =
            Some(identity.clone());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "identity": identity })),
            error: None,
        })
    }

    fn handle_sdk_identity_import_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_import_export") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_import_v2",
                "sdk.capability.identity_import_export",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityImportV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.passphrase.as_deref();
        let _ = parsed.extensions.len();
        let bundle_base64 = match Self::normalize_non_empty(parsed.bundle_base64.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "bundle_base64 must not be empty",
                ))
            }
        };
        let decoded = BASE64_STANDARD.decode(bundle_base64.as_bytes()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "bundle_base64 is invalid")
        })?;

        let parsed_bundle = serde_json::from_slice::<SdkIdentityBundle>(decoded.as_slice()).ok();
        let mut hasher = Sha256::new();
        hasher.update(decoded.as_slice());
        let generated_identity = format!("id-{}", &encode_hex(hasher.finalize())[..16]);
        let mut bundle = parsed_bundle.unwrap_or(SdkIdentityBundle {
            identity: generated_identity.clone(),
            public_key: format!("{generated_identity}-pub"),
            display_name: None,
            capabilities: Vec::new(),
            extensions: JsonMap::new(),
        });
        if Self::normalize_non_empty(bundle.identity.as_str()).is_none() {
            bundle.identity = generated_identity;
        }
        if Self::normalize_non_empty(bundle.public_key.as_str()).is_none() {
            bundle.public_key = format!("{}-pub", bundle.identity);
        }
        self.sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .insert(bundle.identity.clone(), bundle.clone());
        Ok(RpcResponse { id: request.id, result: Some(json!({ "identity": bundle })), error: None })
    }

    fn handle_sdk_identity_export_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_import_export") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_export_v2",
                "sdk.capability.identity_import_export",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityExportV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let identity = match Self::normalize_non_empty(parsed.identity.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "identity must not be empty",
                ))
            }
        };
        let bundle = self
            .sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .get(identity.as_str())
            .cloned();
        let Some(bundle) = bundle else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "identity not found",
            ));
        };
        let raw = serde_json::to_vec(&bundle).map_err(std::io::Error::other)?;
        let bundle_base64 = BASE64_STANDARD.encode(raw);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "bundle": {
                    "bundle_base64": bundle_base64,
                    "passphrase": JsonValue::Null,
                    "extensions": JsonMap::<String, JsonValue>::new(),
                }
            })),
            error: None,
        })
    }

    fn handle_sdk_identity_resolve_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_hash_resolution") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_resolve_v2",
                "sdk.capability.identity_hash_resolution",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityResolveV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let query = match Self::normalize_non_empty(parsed.hash.as_str()) {
            Some(value) => value.to_ascii_lowercase(),
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "hash must not be empty",
                ))
            }
        };
        let identities_guard = self.sdk_identities.lock().expect("sdk_identities mutex poisoned");
        let identity = identities_guard.values().find_map(|bundle| {
            if bundle.identity.eq_ignore_ascii_case(query.as_str()) {
                return Some(bundle.identity.clone());
            }
            if bundle.public_key.to_ascii_lowercase().contains(query.as_str()) {
                return Some(bundle.identity.clone());
            }
            None
        });
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "identity": identity })),
            error: None,
        })
    }

    fn handle_sdk_paper_encode_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.paper_messages") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_paper_encode_v2",
                "sdk.capability.paper_messages",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPaperEncodeV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let message_id = match Self::normalize_non_empty(parsed.message_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "message_id must not be empty",
                ))
            }
        };
        let message = self.store.get_message(message_id.as_str()).map_err(std::io::Error::other)?;
        let Some(message) = message else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "message not found",
            ));
        };
        let envelope = json!({
            "uri": format!("lxm://{}/{}", message.destination, message.id),
            "transient_id": format!("paper-{}", message.id),
            "destination_hint": message.destination,
            "extensions": JsonMap::<String, JsonValue>::new(),
        });
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "envelope": envelope })),
            error: None,
        })
    }

    fn handle_sdk_paper_decode_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.paper_messages") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_paper_decode_v2",
                "sdk.capability.paper_messages",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPaperDecodeV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        if !parsed.uri.starts_with("lxm://") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "paper URI must start with lxm://",
            ));
        }
        let transient_id = parsed.transient_id.unwrap_or_else(|| {
            let mut hasher = Sha256::new();
            hasher.update(parsed.uri.as_bytes());
            format!("paper-{}", encode_hex(hasher.finalize()))
        });
        let duplicate = {
            let mut guard =
                self.paper_ingest_seen.lock().expect("paper_ingest_seen mutex poisoned");
            if guard.contains(transient_id.as_str()) {
                true
            } else {
                guard.insert(transient_id.clone());
                false
            }
        };
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "transient_id": transient_id,
                "duplicate": duplicate,
                "destination_hint": parsed.destination_hint,
            })),
            error: None,
        })
    }

    fn handle_sdk_command_invoke_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.remote_commands") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_command_invoke_v2",
                "sdk.capability.remote_commands",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCommandInvokeV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let command = match Self::normalize_non_empty(parsed.command.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "command must not be empty",
                ))
            }
        };
        let correlation_id = self.next_sdk_domain_id("cmd");
        self.sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned")
            .insert(correlation_id.clone());
        let response = json!({
            "accepted": true,
            "payload": {
                "correlation_id": correlation_id,
                "command": command,
                "target": parsed.target,
                "echo": parsed.payload,
                "timeout_ms": parsed.timeout_ms,
            },
            "extensions": parsed.extensions,
        });
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "response": response })),
            error: None,
        })
    }

    fn handle_sdk_command_reply_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.remote_commands") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_command_reply_v2",
                "sdk.capability.remote_commands",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCommandReplyV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let correlation_id = match Self::normalize_non_empty(parsed.correlation_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "correlation_id must not be empty",
                ))
            }
        };
        let removed = self
            .sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned")
            .remove(correlation_id.as_str());
        if !removed {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "correlation_id not found",
            ));
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "correlation_id": correlation_id,
                "reply_accepted": parsed.accepted,
                "payload": parsed.payload,
            })),
            error: None,
        })
    }

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
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "session_id": session_id })),
            error: None,
        })
    }

}
