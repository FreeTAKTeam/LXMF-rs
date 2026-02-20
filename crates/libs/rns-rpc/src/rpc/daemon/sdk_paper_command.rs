impl RpcDaemon {
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

}
