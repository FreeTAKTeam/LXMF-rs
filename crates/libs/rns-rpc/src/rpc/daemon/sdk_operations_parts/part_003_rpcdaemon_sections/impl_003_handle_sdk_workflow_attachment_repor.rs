impl RpcDaemon {

    pub(super) fn handle_sdk_workflow_attachment_report_publish_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let topic_sync = self.handle_sdk_workflow_topic_sync_v2(RpcRequest {
            id: request.id,
            method: "sdk_workflow_topic_sync_v2".to_owned(),
            params: Some(json!({
                "topic_path": params.get("topic_path").cloned().unwrap_or(JsonValue::Null),
                "metadata": params.get("topic_metadata").cloned().unwrap_or_else(|| json!({})),
                "telemetry_limit": 0,
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if topic_sync.error.is_some() {
            return Ok(topic_sync);
        }
        let topic = topic_sync
            .result
            .unwrap_or(JsonValue::Null)
            .get("workflow")
            .and_then(|workflow| workflow.get("topic"))
            .cloned()
            .unwrap_or(JsonValue::Null);
        let topic_id =
            topic.get("topic_id").and_then(JsonValue::as_str).unwrap_or_default().to_owned();

        let Some(attachment) = params.get("attachment") else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "workflow attachment report requires attachment",
            ));
        };
        let stored = self.handle_sdk_attachment_store_v2(RpcRequest {
            id: request.id,
            method: "sdk_attachment_store_v2".to_owned(),
            params: Some(json!({
                "name": attachment.get("name").cloned().unwrap_or(JsonValue::Null),
                "content_type": attachment.get("content_type").cloned().unwrap_or(JsonValue::Null),
                "bytes_base64": attachment.get("bytes_base64").cloned().unwrap_or(JsonValue::Null),
                "topic_ids": [topic_id],
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if stored.error.is_some() {
            return Ok(stored);
        }
        let attachment_meta = stored
            .result
            .unwrap_or(JsonValue::Null)
            .get("attachment")
            .cloned()
            .unwrap_or(JsonValue::Null);

        let published = self.handle_sdk_topic_publish_v2(RpcRequest {
            id: request.id,
            method: "sdk_topic_publish_v2".to_owned(),
            params: Some(json!({
                "topic_id": topic.get("topic_id").cloned().unwrap_or(JsonValue::Null),
                "correlation_id": params.get("correlation_id").cloned().unwrap_or(JsonValue::Null),
                "payload": {
                    "summary": params.get("summary_payload").cloned().unwrap_or(JsonValue::Null),
                    "attachment_id": attachment_meta.get("attachment_id").cloned().unwrap_or(JsonValue::Null),
                    "attachment_name": attachment_meta.get("name").cloned().unwrap_or(JsonValue::Null),
                    "content_type": attachment_meta.get("content_type").cloned().unwrap_or(JsonValue::Null),
                },
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if published.error.is_some() {
            return Ok(published);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "topic": topic,
                    "attachment": attachment_meta,
                    "published": published.result.unwrap_or(JsonValue::Null),
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_mission_update_send_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let metadata =
            params.get("metadata").and_then(JsonValue::as_object).cloned().unwrap_or_default();
        for key in ["content", "topic_id", "group_id", "file_attachments"] {
            if metadata.contains_key(key) {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "mission metadata cannot override reserved fields",
                ));
            }
        }

        let peer = self.handle_sdk_workflow_peer_ready_v2(RpcRequest {
            id: request.id,
            method: "sdk_workflow_peer_ready_v2".to_owned(),
            params: Some(json!({
                "identity": params.get("peer_identity").cloned().unwrap_or(JsonValue::Null),
                "display_name": params.get("display_name").cloned().unwrap_or(JsonValue::Null),
                "trust_level": params.get("trust_level").cloned().unwrap_or(JsonValue::Null),
                "bootstrap": params.get("bootstrap").cloned().unwrap_or(JsonValue::Bool(true)),
                "announce": params.get("announce").cloned().unwrap_or(JsonValue::Bool(true)),
                "metadata": JsonValue::Object(metadata.clone()),
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if peer.error.is_some() {
            return Ok(peer);
        }
        let peer_payload = peer
            .result
            .unwrap_or(JsonValue::Null)
            .get("workflow")
            .cloned()
            .unwrap_or(JsonValue::Null);

        let topic = if params.get("topic_path").and_then(JsonValue::as_str).is_some() {
            let ensured = self.handle_sdk_workflow_topic_sync_v2(RpcRequest {
                id: request.id,
                method: "sdk_workflow_topic_sync_v2".to_owned(),
                params: Some(json!({
                    "topic_path": params.get("topic_path").cloned().unwrap_or(JsonValue::Null),
                    "telemetry_limit": 0,
                    "metadata": {},
                    "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                })),
            })?;
            if ensured.error.is_some() {
                return Ok(ensured);
            }
            ensured
                .result
                .unwrap_or(JsonValue::Null)
                .get("workflow")
                .and_then(|workflow| workflow.get("topic"))
                .cloned()
        } else {
            None
        };

        let mut attachment_rows = Vec::new();
        if let Some(attachments) = params.get("attachments").and_then(JsonValue::as_array) {
            for attachment in attachments {
                let stored = self.handle_sdk_attachment_store_v2(RpcRequest {
                    id: request.id,
                    method: "sdk_attachment_store_v2".to_owned(),
                    params: Some(json!({
                        "name": attachment.get("name").cloned().unwrap_or(JsonValue::Null),
                        "content_type": attachment.get("content_type").cloned().unwrap_or(JsonValue::Null),
                        "bytes_base64": attachment.get("bytes_base64").cloned().unwrap_or(JsonValue::Null),
                        "topic_ids": topic
                            .as_ref()
                            .and_then(|topic| topic.get("topic_id").cloned())
                            .map(|topic_id| json!([topic_id]))
                            .unwrap_or_else(|| json!([])),
                        "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                    })),
                })?;
                if stored.error.is_some() {
                    return Ok(stored);
                }
                let attachment_meta = stored
                    .result
                    .unwrap_or(JsonValue::Null)
                    .get("attachment")
                    .cloned()
                    .unwrap_or(JsonValue::Null);
                attachment_rows.push(attachment_meta);
            }
        }

        let mut fields = metadata;
        if let Some(topic) = topic.as_ref() {
            if let Some(topic_id) = topic.get("topic_id").cloned() {
                fields.insert("topic_id".to_owned(), topic_id.clone());
                fields.insert("group_id".to_owned(), topic_id);
            }
        }
        if !attachment_rows.is_empty() {
            fields.insert(
                "file_attachments".to_owned(),
                JsonValue::Array(
                    attachment_rows
                        .iter()
                        .map(|attachment| {
                            json!({
                                "attachment_id": attachment.get("attachment_id").cloned().unwrap_or(JsonValue::Null),
                                "name": attachment.get("name").cloned().unwrap_or(JsonValue::Null),
                                "content_type": attachment.get("content_type").cloned().unwrap_or(JsonValue::Null),
                                "byte_len": attachment.get("byte_len").cloned().unwrap_or(JsonValue::Null),
                            })
                        })
                        .collect(),
                ),
            );
        }

        let sent = self.handle_rpc_legacy_messages(RpcRequest {
            id: request.id,
            method: "sdk_send_v2".to_owned(),
            params: Some(json!({
                "id": params
                    .get("idempotency_key")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.next_sdk_domain_id("workflow-mission")),
                "source": self.local_delivery_hash(),
                "destination": params.get("peer_identity").cloned().unwrap_or(JsonValue::Null),
                "title": "",
                "content": params.get("content").cloned().unwrap_or(JsonValue::Null),
                "fields": JsonValue::Object(fields),
            })),
        })?;
        if sent.error.is_some() {
            return Ok(sent);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "peer": peer_payload,
                    "message_id": sent.result.unwrap_or(JsonValue::Null).get("message_id").cloned().unwrap_or(JsonValue::Null),
                    "topic": topic,
                    "attachments": attachment_rows,
                }
            })),
            error: None,
        })
    }
}
