impl RpcDaemon {

    pub(super) fn collection_cursor_index(
        &self,
        cursor: Option<&str>,
        prefix: &str,
    ) -> Result<usize, SdkCursorError> {
        let Some(cursor) = cursor else {
            return Ok(0);
        };
        let cursor = cursor.trim();
        if cursor.is_empty() {
            return Err(SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor must not be empty".to_string(),
            });
        }
        let Some(value) = cursor.strip_prefix(prefix) else {
            return Err(SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor scope does not match method domain".to_string(),
            });
        };
        value.parse::<usize>().map_err(|_| SdkCursorError {
            code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
            message: "cursor index is invalid".to_string(),
        })
    }

    pub(super) fn collection_next_cursor(
        prefix: &str,
        next_index: usize,
        total_items: usize,
    ) -> Option<String> {
        if next_index >= total_items {
            return None;
        }
        Some(format!("{prefix}{next_index}"))
    }

    pub(super) fn record_sdk_cursor_hint(&self, method: &str, response: &RpcResponse) {
        if response.error.is_some() {
            return;
        }
        let Some(next_cursor) = response
            .result
            .as_ref()
            .and_then(JsonValue::as_object)
            .and_then(|result| result.get("next_cursor"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
        else {
            return;
        };

        let hint = SdkCursorHint {
            method: method.to_string(),
            next_cursor: next_cursor.to_string(),
            captured_at_ms: now_millis_u64(),
        };
        let mut guard = self.sdk_cursor_hints.lock().expect("sdk_cursor_hints mutex poisoned");
        guard.insert(method.to_string(), hint);
    }

    pub(super) fn handle_sdk_cursor_hint_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let requested_method = match params.get("method") {
            Some(JsonValue::String(value)) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "method must not be empty",
                    ));
                }
                Some(trimmed.to_string())
            }
            Some(JsonValue::Null) | None => None,
            Some(_) => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "method must be a string",
                ))
            }
        };

        let guard = self.sdk_cursor_hints.lock().expect("sdk_cursor_hints mutex poisoned");
        let result = if let Some(method) = requested_method {
            let hint = guard.get(method.as_str()).cloned();
            json!({
                "method": method,
                "hint": hint,
            })
        } else {
            let hints = guard
                .iter()
                .map(|(method, hint)| (method.clone(), json!(hint)))
                .collect::<JsonMap<String, JsonValue>>();
            json!({ "hints": hints })
        };
        Ok(RpcResponse { id: request.id, result: Some(result), error: None })
    }

    pub(super) fn normalize_non_empty(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    pub(super) fn normalize_voice_state(value: &str) -> Option<&'static str> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "new" => Some("new"),
            "ringing" => Some("ringing"),
            "active" => Some("active"),
            "holding" => Some("holding"),
            "closed" => Some("closed"),
            "failed" => Some("failed"),
            _ => None,
        }
    }

    pub(super) fn voice_state_rank(value: &str) -> u8 {
        match value {
            "new" => 0,
            "ringing" => 1,
            "active" => 2,
            "holding" => 3,
            "closed" | "failed" => 4,
            _ => 0,
        }
    }

    pub(super) fn parse_domain_sequence(id: &str) -> Option<u64> {
        let (_, suffix) = id.rsplit_once('-')?;
        if suffix.len() != 16 {
            return None;
        }
        u64::from_str_radix(suffix, 16).ok()
    }

    pub(super) fn infer_snapshot_domain_sequence(snapshot: &SdkDomainSnapshotV1) -> u64 {
        let mut max_seq = snapshot.next_domain_seq;
        for id in snapshot.topics.keys() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        for id in snapshot.attachments.keys() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        for id in snapshot.markers.keys() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        for id in snapshot.remote_commands.keys() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        for id in snapshot.voice_sessions.keys() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        max_seq
    }

    pub(super) fn default_identity_map(&self) -> HashMap<String, SdkIdentityBundle> {
        let mut identities = HashMap::new();
        identities.insert(
            self.identity_hash.clone(),
            Self::default_sdk_identity(self.identity_hash.as_str()),
        );
        identities
    }

    pub(super) fn build_sdk_domain_snapshot(&self) -> SdkDomainSnapshotV1 {
        let next_domain_seq =
            *self.sdk_next_domain_seq.lock().expect("sdk_next_domain_seq mutex poisoned");
        let config_revision =
            *self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        let runtime_config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let topics = self.sdk_topics.lock().expect("sdk_topics mutex poisoned").clone();
        let topic_order =
            self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned").clone();
        let topic_subscriptions = self
            .sdk_topic_subscriptions
            .lock()
            .expect("sdk_topic_subscriptions mutex poisoned")
            .clone();
        let telemetry_points =
            self.sdk_telemetry_points.lock().expect("sdk_telemetry_points mutex poisoned").clone();
        let attachments =
            self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned").clone();
        let attachment_payloads = self
            .sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .clone();
        let attachment_order =
            self.sdk_attachment_order.lock().expect("sdk_attachment_order mutex poisoned").clone();
        let markers = self.sdk_markers.lock().expect("sdk_markers mutex poisoned").clone();
        let marker_order =
            self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned").clone();
        let identities = self.sdk_identities.lock().expect("sdk_identities mutex poisoned").clone();
        let contacts = self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned").clone();
        let contact_order =
            self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned").clone();
        let active_identity =
            self.sdk_active_identity.lock().expect("sdk_active_identity mutex poisoned").clone();
        let remote_commands =
            self.sdk_remote_commands.lock().expect("sdk_remote_commands mutex poisoned").clone();
        let voice_sessions =
            self.sdk_voice_sessions.lock().expect("sdk_voice_sessions mutex poisoned").clone();

        SdkDomainSnapshotV1 {
            next_domain_seq,
            config_revision,
            runtime_config,
            topics,
            topic_order,
            topic_subscriptions,
            telemetry_points,
            attachments,
            attachment_payloads,
            attachment_order,
            markers,
            marker_order,
            identities,
            contacts,
            contact_order,
            active_identity,
            remote_commands,
            voice_sessions,
        }
    }

    pub(super) fn normalize_sdk_domain_snapshot(
        &self,
        mut snapshot: SdkDomainSnapshotV1,
    ) -> SdkDomainSnapshotV1 {
        snapshot.topic_order.retain(|topic_id| snapshot.topics.contains_key(topic_id));
        snapshot.topic_subscriptions.retain(|topic_id| snapshot.topics.contains_key(topic_id));
        snapshot
            .attachment_order
            .retain(|attachment_id| snapshot.attachments.contains_key(attachment_id));
        snapshot.marker_order.retain(|marker_id| snapshot.markers.contains_key(marker_id));
        snapshot.contact_order.retain(|identity| snapshot.contacts.contains_key(identity));
        snapshot.remote_commands.retain(|correlation_id, _| !correlation_id.is_empty());
        snapshot
            .attachment_payloads
            .retain(|attachment_id, _| snapshot.attachments.contains_key(attachment_id));

        if snapshot.identities.is_empty() {
            snapshot.identities = self.default_identity_map();
        }
        snapshot
            .identities
            .entry(self.identity_hash.clone())
            .or_insert_with(|| Self::default_sdk_identity(self.identity_hash.as_str()));
        let active_identity_valid = snapshot
            .active_identity
            .as_ref()
            .is_some_and(|value| snapshot.identities.contains_key(value));
        if !active_identity_valid {
            let mut identities = snapshot.identities.keys().cloned().collect::<Vec<_>>();
            identities.sort();
            snapshot.active_identity = identities
                .into_iter()
                .find(|identity| identity == self.identity_hash.as_str())
                .or_else(|| snapshot.identities.keys().min().cloned());
        }
        if !snapshot.runtime_config.is_object()
            || self.validate_sdk_runtime_config(&snapshot.runtime_config).is_err()
        {
            snapshot.runtime_config = JsonValue::Object(JsonMap::new());
            snapshot.config_revision = 0;
        }
        snapshot.next_domain_seq = Self::infer_snapshot_domain_sequence(&snapshot);
        snapshot
    }

    pub(super) fn restore_sdk_domain_snapshot(&self) -> Result<(), std::io::Error> {
        let snapshot = self.store.get_sdk_domain_snapshot().map_err(std::io::Error::other)?;
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let parsed: SdkDomainSnapshotV1 =
            serde_json::from_value(snapshot).map_err(std::io::Error::other)?;
        let parsed = self.normalize_sdk_domain_snapshot(parsed);
        let config_revision = parsed.config_revision;
        let runtime_config = parsed.runtime_config.clone();

        *self.sdk_next_domain_seq.lock().expect("sdk_next_domain_seq mutex poisoned") =
            parsed.next_domain_seq;
        *self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned") =
            config_revision;
        *self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned") =
            runtime_config;
        *self.sdk_topics.lock().expect("sdk_topics mutex poisoned") = parsed.topics;
        *self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned") = parsed.topic_order;
        *self.sdk_topic_subscriptions.lock().expect("sdk_topic_subscriptions mutex poisoned") =
            parsed.topic_subscriptions;
        *self.sdk_telemetry_points.lock().expect("sdk_telemetry_points mutex poisoned") =
            parsed.telemetry_points;
        *self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned") = parsed.attachments;
        *self.sdk_attachment_payloads.lock().expect("sdk_attachment_payloads mutex poisoned") =
            parsed.attachment_payloads;
        *self.sdk_attachment_order.lock().expect("sdk_attachment_order mutex poisoned") =
            parsed.attachment_order;
        *self.sdk_markers.lock().expect("sdk_markers mutex poisoned") = parsed.markers;
        *self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned") =
            parsed.marker_order;
        *self.sdk_identities.lock().expect("sdk_identities mutex poisoned") = parsed.identities;
        *self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned") = parsed.contacts;
        *self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned") =
            parsed.contact_order;
        *self.sdk_active_identity.lock().expect("sdk_active_identity mutex poisoned") =
            parsed.active_identity;
        *self.sdk_remote_commands.lock().expect("sdk_remote_commands mutex poisoned") =
            parsed.remote_commands;
        *self.sdk_voice_sessions.lock().expect("sdk_voice_sessions mutex poisoned") =
            parsed.voice_sessions;
        Ok(())
    }

    pub(super) fn lock_and_restore_sdk_domain_snapshot(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ()>, std::io::Error> {
        let guard =
            self.sdk_domain_state_lock.lock().expect("sdk_domain_state_lock mutex poisoned");
        self.restore_sdk_domain_snapshot()?;
        Ok(guard)
    }

    pub(super) fn persist_sdk_domain_snapshot(&self) -> Result<(), std::io::Error> {
        let snapshot = self.build_sdk_domain_snapshot();
        let value = serde_json::to_value(&snapshot).map_err(std::io::Error::other)?;
        self.store.put_sdk_domain_snapshot(&value).map_err(std::io::Error::other)?;
        Ok(())
    }
}
