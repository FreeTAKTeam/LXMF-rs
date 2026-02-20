impl RpcDaemon {
    fn handle_sdk_topic_create_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.topics") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_topic_create_v2",
                "sdk.capability.topics",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkTopicCreateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let topic_path = match parsed.topic_path {
            Some(value) => {
                let normalized = Self::normalize_non_empty(value.as_str());
                if normalized.is_none() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "topic_path must not be empty when provided",
                    ));
                }
                normalized
            }
            None => None,
        };

        let topic_id = self.next_sdk_domain_id("topic");
        let record = SdkTopicRecord {
            topic_id: topic_id.clone(),
            topic_path,
            created_ts_ms: now_millis_u64(),
            metadata: parsed.metadata,
            extensions: parsed.extensions,
        };
        self.sdk_topics
            .lock()
            .expect("sdk_topics mutex poisoned")
            .insert(topic_id.clone(), record.clone());
        self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned").push(topic_id.clone());
        let event = RpcEvent {
            event_type: "sdk_topic_created".to_string(),
            payload: json!({
                "topic_id": topic_id,
                "created_ts_ms": record.created_ts_ms,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(RpcResponse { id: request.id, result: Some(json!({ "topic": record })), error: None })
    }

    fn handle_sdk_topic_get_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.topics") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_topic_get_v2",
                "sdk.capability.topics",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkTopicGetV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
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
        let topic = self
            .sdk_topics
            .lock()
            .expect("sdk_topics mutex poisoned")
            .get(topic_id.as_str())
            .cloned();
        Ok(RpcResponse { id: request.id, result: Some(json!({ "topic": topic })), error: None })
    }

    fn handle_sdk_topic_list_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.topics") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_topic_list_v2",
                "sdk.capability.topics",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkTopicListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let start_index = match self.collection_cursor_index(parsed.cursor.as_deref(), "topic:") {
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
        let order_guard = self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned");
        if start_index > order_guard.len() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "topic cursor is out of range",
            ));
        }
        let topics_guard = self.sdk_topics.lock().expect("sdk_topics mutex poisoned");
        let topics = order_guard
            .iter()
            .skip(start_index)
            .take(limit)
            .filter_map(|topic_id| topics_guard.get(topic_id).cloned())
            .collect::<Vec<_>>();
        let next_index = start_index.saturating_add(topics.len());
        let next_cursor = Self::collection_next_cursor("topic:", next_index, order_guard.len());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "topics": topics,
                "next_cursor": next_cursor,
            })),
            error: None,
        })
    }

    fn handle_sdk_topic_subscribe_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.topic_subscriptions") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_topic_subscribe_v2",
                "sdk.capability.topic_subscriptions",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkTopicSubscriptionV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.cursor.as_deref();
        let _ = parsed.extensions.len();
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
        self.sdk_topic_subscriptions
            .lock()
            .expect("sdk_topic_subscriptions mutex poisoned")
            .insert(topic_id.clone());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "topic_id": topic_id })),
            error: None,
        })
    }

    fn handle_sdk_topic_unsubscribe_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.topic_subscriptions") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_topic_unsubscribe_v2",
                "sdk.capability.topic_subscriptions",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkTopicGetV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
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
        let removed = self
            .sdk_topic_subscriptions
            .lock()
            .expect("sdk_topic_subscriptions mutex poisoned")
            .remove(topic_id.as_str());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": removed, "topic_id": topic_id })),
            error: None,
        })
    }

    fn handle_sdk_topic_publish_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.topic_fanout") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_topic_publish_v2",
                "sdk.capability.topic_fanout",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkTopicPublishV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
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

        let ts_ms = now_millis_u64();
        let mut tags = HashMap::new();
        tags.insert("topic_id".to_string(), topic_id.clone());
        let telemetry = SdkTelemetryPoint {
            ts_ms,
            key: "topic_publish".to_string(),
            value: parsed.payload.clone(),
            unit: None,
            tags,
            extensions: parsed.extensions.clone(),
        };
        self.sdk_telemetry_points
            .lock()
            .expect("sdk_telemetry_points mutex poisoned")
            .push(telemetry);

        let event = RpcEvent {
            event_type: "sdk_topic_published".to_string(),
            payload: json!({
                "topic_id": topic_id,
                "correlation_id": parsed.correlation_id,
                "ts_ms": ts_ms,
                "payload": parsed.payload,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(RpcResponse { id: request.id, result: Some(json!({ "accepted": true })), error: None })
    }

    fn handle_sdk_telemetry_query_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.telemetry_query") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_telemetry_query_v2",
                "sdk.capability.telemetry_query",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkTelemetryQueryV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let mut points =
            self.sdk_telemetry_points.lock().expect("sdk_telemetry_points mutex poisoned").clone();

        if let Some(from_ts_ms) = parsed.from_ts_ms {
            points.retain(|point| point.ts_ms >= from_ts_ms);
        }
        if let Some(to_ts_ms) = parsed.to_ts_ms {
            points.retain(|point| point.ts_ms <= to_ts_ms);
        }
        if let Some(topic_id) = parsed.topic_id {
            points.retain(|point| {
                point.tags.get("topic_id").is_some_and(|current| current == topic_id.as_str())
            });
        }
        if let Some(peer_id) = parsed.peer_id {
            points.retain(|point| {
                point.tags.get("peer_id").is_some_and(|current| current == peer_id.as_str())
            });
        }
        let limit = parsed.limit.unwrap_or(128).clamp(1, 2048);
        if points.len() > limit {
            points.truncate(limit);
        }
        Ok(RpcResponse { id: request.id, result: Some(json!({ "points": points })), error: None })
    }

    fn handle_sdk_telemetry_subscribe_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.telemetry_stream") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_telemetry_subscribe_v2",
                "sdk.capability.telemetry_stream",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkTelemetryQueryV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let event = RpcEvent {
            event_type: "sdk_telemetry_subscribed".to_string(),
            payload: json!({
                "peer_id": parsed.peer_id,
                "topic_id": parsed.topic_id,
                "from_ts_ms": parsed.from_ts_ms,
                "to_ts_ms": parsed.to_ts_ms,
                "limit": parsed.limit,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(RpcResponse { id: request.id, result: Some(json!({ "accepted": true })), error: None })
    }

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
        self.push_event(event.clone());
        let _ = self.events.send(event);
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

    fn handle_sdk_marker_create_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.markers") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_marker_create_v2",
                "sdk.capability.markers",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkMarkerCreateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let label = match Self::normalize_non_empty(parsed.label.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "marker label must not be empty",
                ))
            }
        };
        if !((-90.0..=90.0).contains(&parsed.position.lat)
            && (-180.0..=180.0).contains(&parsed.position.lon))
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "marker coordinates are out of range",
            ));
        }
        if let Some(topic_id) = parsed.topic_id.as_deref() {
            if !self.sdk_topics.lock().expect("sdk_topics mutex poisoned").contains_key(topic_id) {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_NOT_FOUND",
                    "topic not found",
                ));
            }
        }
        let marker_id = self.next_sdk_domain_id("marker");
        let record = SdkMarkerRecord {
            marker_id: marker_id.clone(),
            label,
            position: parsed.position,
            topic_id: parsed.topic_id,
            updated_ts_ms: now_millis_u64(),
            extensions: parsed.extensions,
        };
        self.sdk_markers
            .lock()
            .expect("sdk_markers mutex poisoned")
            .insert(marker_id.clone(), record.clone());
        self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned").push(marker_id);
        Ok(RpcResponse { id: request.id, result: Some(json!({ "marker": record })), error: None })
    }

    fn handle_sdk_marker_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.markers") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_marker_list_v2",
                "sdk.capability.markers",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkMarkerListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let start_index = match self.collection_cursor_index(parsed.cursor.as_deref(), "marker:") {
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
        let order_guard = self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned");
        if start_index > order_guard.len() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "marker cursor is out of range",
            ));
        }
        let markers_guard = self.sdk_markers.lock().expect("sdk_markers mutex poisoned");
        let mut markers = Vec::new();
        let mut next_index = start_index;
        for marker_id in order_guard.iter().skip(start_index) {
            next_index = next_index.saturating_add(1);
            let Some(record) = markers_guard.get(marker_id).cloned() else {
                continue;
            };
            if let Some(topic_id) = parsed.topic_id.as_deref() {
                if record.topic_id.as_deref() != Some(topic_id) {
                    continue;
                }
            }
            markers.push(record);
            if markers.len() >= limit {
                break;
            }
        }
        let next_cursor = Self::collection_next_cursor("marker:", next_index, order_guard.len());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "markers": markers, "next_cursor": next_cursor })),
            error: None,
        })
    }

    fn handle_sdk_marker_update_position_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.markers") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_marker_update_position_v2",
                "sdk.capability.markers",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkMarkerUpdatePositionV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let marker_id = match Self::normalize_non_empty(parsed.marker_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "marker_id must not be empty",
                ))
            }
        };
        if !((-90.0..=90.0).contains(&parsed.position.lat)
            && (-180.0..=180.0).contains(&parsed.position.lon))
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "marker coordinates are out of range",
            ));
        }
        let mut markers = self.sdk_markers.lock().expect("sdk_markers mutex poisoned");
        let Some(record) = markers.get_mut(marker_id.as_str()) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "marker not found",
            ));
        };
        record.position = parsed.position;
        record.updated_ts_ms = now_millis_u64();
        record.extensions = parsed.extensions;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "marker": record.clone() })),
            error: None,
        })
    }

    fn handle_sdk_marker_delete_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.markers") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_marker_delete_v2",
                "sdk.capability.markers",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkMarkerDeleteV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let marker_id = match Self::normalize_non_empty(parsed.marker_id.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "marker_id must not be empty",
                ))
            }
        };
        let removed = self
            .sdk_markers
            .lock()
            .expect("sdk_markers mutex poisoned")
            .remove(marker_id.as_str())
            .is_some();
        self.sdk_marker_order
            .lock()
            .expect("sdk_marker_order mutex poisoned")
            .retain(|current| current != marker_id.as_str());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": removed, "marker_id": marker_id })),
            error: None,
        })
    }

}
