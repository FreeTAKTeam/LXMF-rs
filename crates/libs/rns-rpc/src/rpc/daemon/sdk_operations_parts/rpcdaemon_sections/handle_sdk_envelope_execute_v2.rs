impl RpcDaemon {

    pub(super) fn handle_sdk_envelope_execute_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkEnvelopeExecuteV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let operation_id = match Self::normalize_non_empty(parsed.operation_id.as_str()) {
            Some(value) => value,
            None => return Ok(self.envelope_invalid(request.id, "operation_id must not be empty")),
        };
        let kind = parsed.kind.trim().to_ascii_lowercase();
        if !matches!(kind.as_str(), "query" | "command") {
            return Ok(self.envelope_invalid(request.id, "kind must be query or command"));
        }

        let spec = self.operation_spec(operation_id.as_str());
        let (canonical_id, rpc_method) = if let Some(spec) = spec {
            if spec.kind != kind {
                return Ok(self.envelope_invalid(
                    request.id,
                    "envelope kind does not match registered operation kind",
                ));
            }
            (spec.id, spec.rpc_method)
        } else if kind == "command" {
            (operation_id, "sdk_command_invoke_v2")
        } else {
            return Ok(self.envelope_invalid(request.id, "unknown operation id"));
        };

        let delegated_params = match rpc_method {
            "sdk_send_v2" => parsed.payload,
            "sdk_snapshot_v2" => json!({}),
            "sdk_cursor_hint_v2" => parsed.payload,
            "sdk_status_v2" => json!({
                "message_id": parsed.payload.get("message_id").and_then(JsonValue::as_str),
            }),
            "sdk_poll_events_v2" => json!({
                "cursor": parsed.payload.get("cursor").cloned().unwrap_or(JsonValue::Null),
                "max": parsed.payload.get("max").cloned().unwrap_or(JsonValue::from(32_u64)),
            }),
            "sdk_identity_list_v2" => json!({}),
            "sdk_identity_announce_now_v2" => json!({}),
            "sdk_identity_presence_list_v2" => parsed.payload,
            "sdk_identity_contact_list_v2" => parsed.payload,
            "sdk_identity_contact_update_v2" => parsed.payload,
            "sdk_identity_bootstrap_v2" => parsed.payload,
            "sdk_workflow_peer_ready_v2" => parsed.payload,
            "sdk_workflow_topic_sync_v2" => parsed.payload,
            "sdk_workflow_attachment_report_publish_v2" => parsed.payload,
            "sdk_workflow_mission_update_send_v2" => parsed.payload,
            "sdk_topic_create_v2" => parsed.payload,
            "sdk_topic_get_v2" => json!({
                "topic_id": parsed.payload,
            }),
            "sdk_topic_list_v2" => parsed.payload,
            "sdk_topic_subscribe_v2" => parsed.payload,
            "sdk_topic_unsubscribe_v2" => json!({
                "topic_id": parsed.payload,
            }),
            "sdk_topic_publish_v2" => parsed.payload,
            "sdk_telemetry_query_v2" => parsed.payload,
            "sdk_telemetry_subscribe_v2" => parsed.payload,
            "sdk_attachment_store_v2" => parsed.payload,
            "sdk_attachment_get_v2" => json!({
                "attachment_id": parsed.payload,
            }),
            "sdk_attachment_list_v2" => parsed.payload,
            "sdk_attachment_delete_v2" => json!({
                "attachment_id": parsed.payload,
            }),
            "sdk_attachment_associate_topic_v2" => parsed.payload,
            "sdk_attachment_upload_start_v2" => parsed.payload,
            "sdk_attachment_upload_chunk_v2" => parsed.payload,
            "sdk_attachment_upload_commit_v2" => parsed.payload,
            "sdk_attachment_download_chunk_v2" => parsed.payload,
            "sdk_marker_create_v2" => parsed.payload,
            "sdk_marker_list_v2" => parsed.payload,
            "sdk_marker_update_position_v2" => parsed.payload,
            "sdk_marker_delete_v2" => parsed.payload,
            "sdk_voice_session_open_v2" => parsed.payload,
            "sdk_voice_session_update_v2" => parsed.payload,
            "sdk_voice_session_close_v2" => parsed.payload,
            "list_messages" => json!({
                "limit": parsed.payload.get("limit").cloned().unwrap_or(JsonValue::from(100_u64)),
                "offset": parsed.payload.get("offset").cloned().unwrap_or(JsonValue::from(0_u64)),
            }),
            "status" => json!({}),
            "sdk_command_invoke_v2" => json!({
                "command": canonical_id,
                "target": parsed.target,
                "payload": parsed.payload,
                "timeout_ms": parsed.timeout_ms,
                "extensions": parsed.extensions,
            }),
            _ => JsonValue::Null,
        };

        let delegated =
            self.envelope_execute_delegated(request.id, rpc_method, delegated_params)?;
        if let Some(error) = delegated.error {
            return Ok(RpcResponse { id: request.id, result: None, error: Some(error) });
        }
        let delegated_payload = delegated
            .result
            .and_then(|value| value.get("response").cloned())
            .unwrap_or(JsonValue::Null);
        let accepted =
            delegated_payload.get("accepted").and_then(JsonValue::as_bool).unwrap_or(true);
        let response_correlation_id = parsed.correlation_id;
        let extensions = delegated_payload
            .get("extensions")
            .and_then(JsonValue::as_object)
            .cloned()
            .unwrap_or_default();
        let payload = delegated_payload.get("payload").cloned().unwrap_or(delegated_payload);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "response": {
                    "operation_id": canonical_id,
                    "kind": "result",
                    "accepted": accepted,
                    "correlation_id": response_correlation_id,
                    "payload": payload,
                    "extensions": extensions,
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_peer_ready_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let Some(identity) = params.get("identity").and_then(JsonValue::as_str) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "workflow peer ready requires identity",
            ));
        };
        let announce = params.get("announce").and_then(JsonValue::as_bool).unwrap_or(true);
        let bootstrap = params.get("bootstrap").and_then(JsonValue::as_bool).unwrap_or(true);

        let mut existing_contact = None;
        let mut cursor = None;
        loop {
            let listed = self.handle_sdk_identity_contact_list_v2(RpcRequest {
                id: request.id,
                method: "sdk_identity_contact_list_v2".to_owned(),
                params: Some(json!({
                    "cursor": cursor,
                    "limit": 100,
                })),
            })?;
            if listed.error.is_some() {
                return Ok(listed);
            }
            let result = listed.result.unwrap_or(JsonValue::Null);
            let contact_list = result.get("contact_list").cloned().unwrap_or(JsonValue::Null);
            if let Some(found) = contact_list
                .get("contacts")
                .and_then(JsonValue::as_array)
                .and_then(|contacts| {
                    contacts.iter().find(|contact| {
                        contact.get("identity").and_then(JsonValue::as_str) == Some(identity)
                    })
                })
                .cloned()
            {
                existing_contact = Some(found);
                break;
            }
            match contact_list.get("next_cursor").and_then(JsonValue::as_str) {
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_owned()),
                _ => break,
            }
        }

        let announced = if announce {
            let announce_response = self.handle_sdk_identity_announce_now_v2(RpcRequest {
                id: request.id,
                method: "sdk_identity_announce_now_v2".to_owned(),
                params: Some(json!({})),
            })?;
            if announce_response.error.is_some() {
                return Ok(announce_response);
            }
            true
        } else {
            false
        };

        let contact = if let Some(contact) = existing_contact {
            (contact, false)
        } else {
            let created = if bootstrap {
                self.handle_sdk_identity_bootstrap_v2(RpcRequest {
                    id: request.id,
                    method: "sdk_identity_bootstrap_v2".to_owned(),
                    params: Some(json!({
                        "identity": identity,
                        "auto_sync": true,
                        "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                    })),
                })?
            } else {
                self.handle_sdk_identity_contact_update_v2(RpcRequest {
                    id: request.id,
                    method: "sdk_identity_contact_update_v2".to_owned(),
                    params: Some(json!({
                        "identity": identity,
                        "display_name": params.get("display_name").cloned().unwrap_or(JsonValue::Null),
                        "trust_level": params.get("trust_level").cloned().unwrap_or(JsonValue::Null),
                        "bootstrap": false,
                        "metadata": params.get("metadata").cloned().unwrap_or_else(|| json!({})),
                        "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                    })),
                })?
            };
            if created.error.is_some() {
                return Ok(created);
            }
            (
                created
                    .result
                    .unwrap_or(JsonValue::Null)
                    .get("contact")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
                true,
            )
        };

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "identity": identity,
                    "contact": contact.0,
                    "was_created": contact.1,
                    "announced": announced,
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_topic_sync_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let Some(topic_path) = params.get("topic_path").and_then(JsonValue::as_str) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "workflow topic sync requires topic_path",
            ));
        };

        let mut topic = None;
        let mut cursor = None;
        loop {
            let listed = self.handle_sdk_topic_list_v2(RpcRequest {
                id: request.id,
                method: "sdk_topic_list_v2".to_owned(),
                params: Some(json!({
                    "cursor": cursor,
                    "limit": 100,
                })),
            })?;
            if listed.error.is_some() {
                return Ok(listed);
            }
            let result = listed.result.unwrap_or(JsonValue::Null);
            if let Some(found) = result
                .get("topics")
                .and_then(JsonValue::as_array)
                .and_then(|topics| {
                    topics.iter().find(|topic| {
                        topic.get("topic_path").and_then(JsonValue::as_str) == Some(topic_path)
                    })
                })
                .cloned()
            {
                topic = Some((found, false));
                break;
            }
            match result.get("next_cursor").and_then(JsonValue::as_str) {
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_owned()),
                _ => break,
            }
        }

        let (topic, was_created) = if let Some(topic) = topic {
            topic
        } else {
            let created = self.handle_sdk_topic_create_v2(RpcRequest {
                id: request.id,
                method: "sdk_topic_create_v2".to_owned(),
                params: Some(json!({
                    "topic_path": topic_path,
                    "metadata": params.get("metadata").cloned().unwrap_or_else(|| json!({})),
                    "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                })),
            })?;
            if created.error.is_some() {
                return Ok(created);
            }
            (
                created
                    .result
                    .unwrap_or(JsonValue::Null)
                    .get("topic")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
                true,
            )
        };

        let topic_id =
            topic.get("topic_id").and_then(JsonValue::as_str).unwrap_or_default().to_owned();

        let subscribed = self.handle_sdk_topic_subscribe_v2(RpcRequest {
            id: request.id,
            method: "sdk_topic_subscribe_v2".to_owned(),
            params: Some(json!({
                "topic_id": topic_id,
            })),
        })?;
        if subscribed.error.is_some() {
            return Ok(subscribed);
        }

        let telemetry = self.handle_sdk_telemetry_query_v2(RpcRequest {
            id: request.id,
            method: "sdk_telemetry_query_v2".to_owned(),
            params: Some(json!({
                "topic_id": topic_id,
                "limit": params.get("telemetry_limit").cloned().unwrap_or(JsonValue::from(100_u64)),
            })),
        })?;
        if telemetry.error.is_some() {
            return Ok(telemetry);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "topic": topic,
                    "was_created": was_created,
                    "subscribed": subscribed.result.unwrap_or(JsonValue::Null).get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
                    "telemetry": telemetry.result.unwrap_or(JsonValue::Null).get("points").cloned().unwrap_or_else(|| json!([])),
                }
            })),
            error: None,
        })
    }
}
