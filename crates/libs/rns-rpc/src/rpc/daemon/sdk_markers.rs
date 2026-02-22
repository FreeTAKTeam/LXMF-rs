impl RpcDaemon {
    fn marker_revision_conflict_response(
        &self,
        request_id: u64,
        marker_id: &str,
        expected_revision: u64,
        observed_revision: u64,
    ) -> RpcResponse {
        let mut error = RpcError::new("SDK_RUNTIME_CONFLICT", "marker revision mismatch");
        let mut details = JsonMap::new();
        details.insert("domain".to_string(), JsonValue::String("marker".to_string()));
        details.insert("marker_id".to_string(), JsonValue::String(marker_id.to_string()));
        details.insert(
            "expected_revision".to_string(),
            JsonValue::Number(serde_json::Number::from(expected_revision)),
        );
        details.insert(
            "observed_revision".to_string(),
            JsonValue::Number(serde_json::Number::from(observed_revision)),
        );
        error.details = Some(Box::new(details));
        RpcResponse { id: request_id, result: None, error: Some(error) }
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
            revision: 1,
            updated_ts_ms: now_millis_u64(),
            extensions: parsed.extensions,
        };
        self.sdk_markers
            .lock()
            .expect("sdk_markers mutex poisoned")
            .insert(marker_id.clone(), record.clone());
        self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned").push(marker_id);
        self.persist_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        if parsed.expected_revision == 0 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "expected_revision must be greater than 0",
            ));
        }
        if !((-90.0..=90.0).contains(&parsed.position.lat)
            && (-180.0..=180.0).contains(&parsed.position.lon))
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "marker coordinates are out of range",
            ));
        }
        let marker = {
            let mut markers = self.sdk_markers.lock().expect("sdk_markers mutex poisoned");
            let Some(record) = markers.get_mut(marker_id.as_str()) else {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_NOT_FOUND",
                    "marker not found",
                ));
            };
            if record.revision != parsed.expected_revision {
                return Ok(self.marker_revision_conflict_response(
                    request.id,
                    marker_id.as_str(),
                    parsed.expected_revision,
                    record.revision,
                ));
            }
            record.position = parsed.position;
            record.updated_ts_ms = now_millis_u64();
            record.revision = record.revision.saturating_add(1);
            record.extensions = parsed.extensions;
            record.clone()
        };
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "marker": marker })),
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        if parsed.expected_revision == 0 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "expected_revision must be greater than 0",
            ));
        }
        let removed = {
            let mut markers = self.sdk_markers.lock().expect("sdk_markers mutex poisoned");
            match markers.get(marker_id.as_str()) {
                Some(existing) => {
                    if existing.revision != parsed.expected_revision {
                        return Ok(self.marker_revision_conflict_response(
                            request.id,
                            marker_id.as_str(),
                            parsed.expected_revision,
                            existing.revision,
                        ));
                    }
                    markers.remove(marker_id.as_str());
                    true
                }
                None => false,
            }
        };
        if removed {
            self.sdk_marker_order
                .lock()
                .expect("sdk_marker_order mutex poisoned")
                .retain(|current| current != marker_id.as_str());
            self.persist_sdk_domain_snapshot()?;
        }
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": removed, "marker_id": marker_id })),
            error: None,
        })
    }
}
