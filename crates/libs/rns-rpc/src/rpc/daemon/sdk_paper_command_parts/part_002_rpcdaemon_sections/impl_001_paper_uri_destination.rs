impl RpcDaemon {

    fn paper_uri_destination(uri: &str) -> Option<String> {
        let encoded = uri.strip_prefix("lxm://")?;
        if let Some((destination, _)) = encoded.split_once('/') {
            return Self::normalize_non_empty(destination);
        }

        let paper_bytes =
            URL_SAFE_NO_PAD.decode(encoded).or_else(|_| URL_SAFE.decode(encoded)).ok()?;
        (paper_bytes.len() >= 16).then(|| encode_hex(&paper_bytes[..16]))
    }

    pub(super) fn sdk_command_event_payload_summary(payload: &JsonValue) -> JsonValue {
        let byte_len = payload.to_string().len();
        match payload {
            JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => payload.clone(),
            JsonValue::String(value) if byte_len <= 512 => JsonValue::String(value.clone()),
            JsonValue::String(value) => {
                let preview = value.chars().take(128).collect::<String>();
                json!({
                    "kind": "string",
                    "byte_len": byte_len,
                    "preview": preview,
                    "truncated": true,
                })
            }
            JsonValue::Array(items) => json!({
                "kind": "array",
                "len": items.len(),
                "byte_len": byte_len,
            }),
            JsonValue::Object(map) => json!({
                "kind": "object",
                "keys": map.keys().take(16).cloned().collect::<Vec<_>>(),
                "byte_len": byte_len,
                "truncated": byte_len > 512,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_sdk_command_update(
        &self,
        correlation_id: &str,
        event_type: &str,
        accepted: Option<bool>,
        payload: Option<JsonValue>,
        extensions: JsonMap<String, JsonValue>,
        delivery_state: Option<String>,
        command_state: Option<&str>,
    ) -> Result<Option<SdkRemoteCommandRecord>, std::io::Error> {
        let now_ms = u64::try_from(now_i64()).unwrap_or(0);
        let updated_session = {
            let mut sessions =
                self.sdk_remote_commands.lock().expect("sdk_remote_commands mutex poisoned");
            let Some(session) = sessions.get_mut(correlation_id) else {
                return Ok(None);
            };
            session.updated_at_ms = now_ms;
            if let Some(accepted) = accepted {
                session.accepted = Some(accepted);
            }
            if let Some(payload) = payload {
                session.response_payload = Some(payload);
            }
            if let Some(delivery_state) = delivery_state {
                session.delivery_state = Some(delivery_state);
            }
            if let Some(command_state) = command_state {
                session.command_state = command_state.to_owned();
            }
            session.extensions.extend(extensions);
            session.clone()
        };
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: event_type.into(),
            payload: json!({
                "command_id": updated_session.command_id,
                "correlation_id": updated_session.correlation_id,
                "command": updated_session.command,
                "target": updated_session.target,
                "delivery_state": updated_session.delivery_state,
                "command_state": updated_session.command_state,
                "accepted": updated_session.accepted,
                "response_payload": updated_session
                    .response_payload
                    .as_ref()
                    .map(Self::sdk_command_event_payload_summary)
                    .unwrap_or(JsonValue::Null),
            }),
        });
        Ok(Some(updated_session))
    }

    pub(super) fn inbound_sdk_command_update(
        record: &MessageRecord,
    ) -> Option<InboundSdkCommandUpdate> {
        let fields = record.fields.as_ref()?.as_object()?;
        let command = fields.get("sdk_command")?.as_object()?;
        let correlation_id = Self::normalize_non_empty(command.get("correlation_id")?.as_str()?)?;
        let event = Self::normalize_non_empty(command.get("event")?.as_str()?)?;
        let accepted = command.get("accepted").and_then(JsonValue::as_bool);
        let payload = command.get("payload").cloned();
        let extensions =
            command.get("extensions").and_then(JsonValue::as_object).cloned().unwrap_or_default();
        match event.as_str() {
            "receipt_acknowledged" => Some((
                correlation_id,
                "command.receipt_acknowledged",
                accepted,
                payload,
                extensions,
                Some("acknowledged".to_owned()),
                None,
            )),
            "processing_started" => Some((
                correlation_id,
                "command.processing_started",
                accepted,
                payload,
                extensions,
                None,
                Some("processing"),
            )),
            "progress" => Some((
                correlation_id,
                "command.progress",
                accepted,
                payload,
                extensions,
                None,
                Some("processing"),
            )),
            "completed" => Some((
                correlation_id,
                "command.completed",
                Some(accepted.unwrap_or(true)),
                payload,
                extensions,
                None,
                Some("completed"),
            )),
            "failed" => Some((
                correlation_id,
                "command.failed",
                Some(accepted.unwrap_or(false)),
                payload,
                extensions,
                None,
                Some("failed"),
            )),
            _ => None,
        }
    }

    pub(crate) fn correlate_inbound_sdk_command(
        &self,
        record: &MessageRecord,
    ) -> Result<bool, std::io::Error> {
        let Some((
            correlation_id,
            event_type,
            accepted,
            payload,
            extensions,
            delivery_state,
            command_state,
        )) = Self::inbound_sdk_command_update(record)
        else {
            return Ok(false);
        };
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        Ok(self
            .apply_sdk_command_update(
                correlation_id.as_str(),
                event_type,
                accepted,
                payload,
                extensions,
                delivery_state,
                command_state,
            )?
            .is_some())
    }

    pub(super) fn sdk_remote_command_record_to_value(record: &SdkRemoteCommandRecord) -> JsonValue {
        json!({
            "command_id": record.command_id,
            "correlation_id": record.correlation_id,
            "command": record.command,
            "target": record.target,
            "timeout_ms": record.timeout_ms,
            "delivery_state": record.delivery_state,
            "command_state": record.command_state,
            "created_at_ms": record.created_at_ms,
            "updated_at_ms": record.updated_at_ms,
            "request_payload": record.request_payload,
            "response_payload": record.response_payload,
            "accepted": record.accepted,
            "extensions": record.extensions,
        })
    }

    pub(super) fn handle_sdk_paper_encode_v2(
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        let envelope = match self.outbound_bridge.as_ref() {
            Some(bridge) => bridge.encode_paper(&message)?.map(|envelope| {
                json!({
                    "uri": envelope.uri,
                    "transient_id": envelope.transient_id,
                    "destination_hint": envelope.destination_hint,
                    "extensions": envelope.extensions,
                })
            }),
            None => None,
        }
        .unwrap_or_else(|| {
            json!({
                "uri": format!("lxm://{}/{}", message.destination, message.id),
                "transient_id": format!("paper-{}", message.id),
                "destination_hint": message.destination,
                "extensions": JsonMap::<String, JsonValue>::new(),
            })
        });
        {
            let paper_status = "sent: paper".to_string();
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let existing_status = self
                .store
                .get_message(message_id.as_str())
                .map_err(std::io::Error::other)?
                .and_then(|stored| stored.receipt_status);
            let should_mark_generated = existing_status.as_deref().map_or(true, |status| {
                let normalized = status.trim().to_ascii_lowercase();
                !normalized.starts_with("sent") && !Self::is_terminal_receipt_status(status)
            });
            if should_mark_generated {
                self.store
                    .update_receipt_status(message_id.as_str(), paper_status.as_str())
                    .map_err(std::io::Error::other)?;
                self.append_delivery_trace(message_id.as_str(), paper_status);
            }
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "envelope": envelope })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_paper_decode_v2(
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        let uri_payload = parsed.uri.trim_start_matches("lxm://");
        if uri_payload.is_empty()
            || uri_payload
                .split_once('/')
                .is_some_and(|(destination, _)| destination.trim().is_empty())
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "paper URI must include a destination",
            ));
        }
        let bridged_decode = match self.outbound_bridge.as_ref() {
            Some(bridge) => bridge.decode_paper_uri(parsed.uri.as_str())?,
            None => None,
        };
        let destination = bridged_decode
            .as_ref()
            .map(|outcome| outcome.destination_hint.clone())
            .or(parsed.destination_hint)
            .or_else(|| Self::paper_uri_destination(parsed.uri.as_str()));
        let Some(destination) = destination else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "paper URI must include a destination",
            ));
        };
        let bytes_len = bridged_decode
            .as_ref()
            .and_then(|outcome| outcome.raw_lxmf_bytes.as_ref())
            .map_or_else(|| parsed.uri.len(), Vec::len);
        let transient_id = parsed
            .transient_id
            .or_else(|| bridged_decode.as_ref().map(|outcome| outcome.transient_id.clone()))
            .unwrap_or_else(|| {
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
        if !duplicate {
            if let Some(outcome) = bridged_decode.as_ref() {
                if let Some(record) = outcome.record.clone() {
                    if let Some(raw_lxmf_bytes) = outcome.raw_lxmf_bytes.as_ref() {
                        self.accept_inbound_with_raw(record, raw_lxmf_bytes)?;
                    } else {
                        self.accept_inbound(record)?;
                    }
                }
            }
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "transient_id": transient_id,
                "duplicate": duplicate,
                "destination": destination,
                "destination_hint": destination,
                "bytes_len": bytes_len,
            })),
            error: None,
        })
    }
}
