impl RpcDaemon {
    fn handle_sdk_attachment_store_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachments") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_store_v2",
                "sdk.capability.attachments",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentStoreV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let name = match Self::normalize_non_empty(parsed.name.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment name must not be empty",
                ))
            }
        };
        let content_type = match Self::normalize_non_empty(parsed.content_type.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment content_type must not be empty",
                ))
            }
        };
        let decoded_bytes =
            BASE64_STANDARD.decode(parsed.bytes_base64.as_bytes()).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "attachment bytes_base64 is invalid",
                )
            })?;
        if let Some(missing_topic) = parsed.topic_ids.iter().find(|topic_id| {
            !self
                .sdk_topics
                .lock()
                .expect("sdk_topics mutex poisoned")
                .contains_key(topic_id.as_str())
        }) {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                &format!("attachment references unknown topic_id '{missing_topic}'"),
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(decoded_bytes.as_slice());
        let attachment_id = self.next_sdk_domain_id("attachment");
        let record = SdkAttachmentRecord {
            attachment_id: attachment_id.clone(),
            name,
            content_type,
            byte_len: decoded_bytes.len() as u64,
            checksum_sha256: encode_hex(hasher.finalize()),
            created_ts_ms: now_millis_u64(),
            expires_ts_ms: parsed.expires_ts_ms,
            topic_ids: parsed.topic_ids,
            extensions: parsed.extensions,
        };
        self.sdk_attachments
            .lock()
            .expect("sdk_attachments mutex poisoned")
            .insert(attachment_id.clone(), record.clone());
        self.sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .insert(attachment_id.clone(), parsed.bytes_base64);
        self.sdk_attachment_order
            .lock()
            .expect("sdk_attachment_order mutex poisoned")
            .push(attachment_id.clone());
        let event = RpcEvent {
            event_type: "sdk_attachment_stored".to_string(),
            payload: json!({
                "attachment_id": attachment_id,
                "byte_len": record.byte_len,
            }),
        };
        self.publish_event(event);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "attachment": record })),
            error: None,
        })
    }

    fn handle_sdk_attachment_get_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachments") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_get_v2",
                "sdk.capability.attachments",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentRefV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let attachment_id = match Self::normalize_non_empty(parsed.attachment_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment_id must not be empty",
                ))
            }
        };
        let attachment = self
            .sdk_attachments
            .lock()
            .expect("sdk_attachments mutex poisoned")
            .get(attachment_id.as_str())
            .cloned();
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "attachment": attachment })),
            error: None,
        })
    }

    fn handle_sdk_attachment_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachments") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_list_v2",
                "sdk.capability.attachments",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkAttachmentListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let start_index =
            match self.collection_cursor_index(parsed.cursor.as_deref(), "attachment:") {
                Ok(index) => index,
                Err(error) => {
                    return Ok(self.sdk_error_response(
                        request.id,
                        error.code.as_str(),
                        error.message.as_str(),
                    ))
                }
            };
        let limit = parsed.limit.unwrap_or(100).clamp(1, 500);
        let order_guard =
            self.sdk_attachment_order.lock().expect("sdk_attachment_order mutex poisoned");
        if start_index > order_guard.len() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "attachment cursor is out of range",
            ));
        }
        let attachments_guard =
            self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned");
        let mut attachments = Vec::new();
        let mut next_index = start_index;
        for attachment_id in order_guard.iter().skip(start_index) {
            next_index = next_index.saturating_add(1);
            let Some(record) = attachments_guard.get(attachment_id).cloned() else {
                continue;
            };
            if let Some(topic_id) = parsed.topic_id.as_deref() {
                if !record.topic_ids.iter().any(|current| current == topic_id) {
                    continue;
                }
            }
            attachments.push(record);
            if attachments.len() >= limit {
                break;
            }
        }
        let next_cursor =
            Self::collection_next_cursor("attachment:", next_index, order_guard.len());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "attachments": attachments,
                "next_cursor": next_cursor,
            })),
            error: None,
        })
    }

    fn handle_sdk_attachment_delete_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachment_delete") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_delete_v2",
                "sdk.capability.attachment_delete",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentRefV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let attachment_id = match Self::normalize_non_empty(parsed.attachment_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment_id must not be empty",
                ))
            }
        };
        let removed = self
            .sdk_attachments
            .lock()
            .expect("sdk_attachments mutex poisoned")
            .remove(attachment_id.as_str())
            .is_some();
        self.sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .remove(attachment_id.as_str());
        self.sdk_attachment_order
            .lock()
            .expect("sdk_attachment_order mutex poisoned")
            .retain(|current| current != attachment_id.as_str());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": removed, "attachment_id": attachment_id })),
            error: None,
        })
    }

    fn handle_sdk_attachment_download_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachments") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_download_v2",
                "sdk.capability.attachments",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentRefV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let attachment_id = match Self::normalize_non_empty(parsed.attachment_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment_id must not be empty",
                ))
            }
        };
        let payload = self
            .sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .get(attachment_id.as_str())
            .cloned();
        if payload.is_none() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "attachment not found",
            ));
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "attachment_id": attachment_id,
                "bytes_base64": payload,
            })),
            error: None,
        })
    }

    fn handle_sdk_attachment_associate_topic_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.attachments") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_attachment_associate_topic_v2",
                "sdk.capability.attachments",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkAttachmentAssociateTopicV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let attachment_id = match Self::normalize_non_empty(parsed.attachment_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "attachment_id must not be empty",
                ))
            }
        };
        let topic_id = match Self::normalize_non_empty(parsed.topic_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "topic_id must not be empty",
                ))
            }
        };
        if !self
            .sdk_topics
            .lock()
            .expect("sdk_topics mutex poisoned")
            .contains_key(topic_id.as_str())
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "topic not found",
            ));
        }
        let mut attachments = self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned");
        let Some(record) = attachments.get_mut(attachment_id.as_str()) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "attachment not found",
            ));
        };
        if !record.topic_ids.iter().any(|current| current == topic_id.as_str()) {
            record.topic_ids.push(topic_id.clone());
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(
                json!({ "accepted": true, "attachment_id": attachment_id, "topic_id": topic_id }),
            ),
            error: None,
        })
    }
}
