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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.persist_sdk_domain_snapshot()?;
        let event = RpcEvent {
            event_type: "sdk_topic_created".to_string(),
            payload: json!({
                "topic_id": topic_id,
                "created_ts_ms": record.created_ts_ms,
            }),
        };
        self.publish_event(event);
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.persist_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.persist_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.persist_sdk_domain_snapshot()?;

        let event = RpcEvent {
            event_type: "sdk_topic_published".to_string(),
            payload: json!({
                "topic_id": topic_id,
                "correlation_id": parsed.correlation_id,
                "ts_ms": ts_ms,
                "payload": parsed.payload,
            }),
        };
        self.publish_event(event);
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.publish_event(event);
        Ok(RpcResponse { id: request.id, result: Some(json!({ "accepted": true })), error: None })
    }
}
