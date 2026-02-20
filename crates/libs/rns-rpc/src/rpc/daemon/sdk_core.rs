impl RpcDaemon {
    fn handle_sdk_negotiate_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkNegotiateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let active_contract_version = parsed
            .supported_contract_versions
            .iter()
            .copied()
            .filter(|version| *version == 2)
            .max();

        let Some(active_contract_version) = active_contract_version else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                "no compatible contract version",
            ));
        };

        let profile = parsed.config.profile.trim().to_ascii_lowercase();
        if !matches!(profile.as_str(), "desktop-full" | "desktop-local-runtime") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                "profile is not supported by the rpc backend",
            ));
        }

        let bind_mode =
            parsed.config.bind_mode.as_deref().unwrap_or("local_only").trim().to_ascii_lowercase();
        if !matches!(bind_mode.as_str(), "local_only" | "remote") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "bind_mode must be local_only or remote",
            ));
        }

        let auth_mode = parsed
            .config
            .auth_mode
            .as_deref()
            .unwrap_or("local_trusted")
            .trim()
            .to_ascii_lowercase();
        if !matches!(auth_mode.as_str(), "local_trusted" | "token" | "mtls") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "auth_mode must be local_trusted, token, or mtls",
            ));
        }
        if bind_mode == "remote" && !matches!(auth_mode.as_str(), "token" | "mtls") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_SECURITY_REMOTE_BIND_DISALLOWED",
                "remote bind mode requires token or mtls auth mode",
            ));
        }
        if bind_mode == "local_only" && auth_mode != "local_trusted" {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_SECURITY_AUTH_REQUIRED",
                "local_only bind mode requires local_trusted auth mode",
            ));
        }

        let overflow_policy = parsed
            .config
            .overflow_policy
            .as_deref()
            .unwrap_or("reject")
            .trim()
            .to_ascii_lowercase();
        if !matches!(overflow_policy.as_str(), "reject" | "drop_oldest" | "block") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "overflow_policy must be reject, drop_oldest, or block",
            ));
        }
        if overflow_policy == "block" && parsed.config.block_timeout_ms.is_none() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "overflow_policy=block requires block_timeout_ms",
            ));
        }

        match auth_mode.as_str() {
            "token" => {
                let Some(token_auth) = parsed
                    .config
                    .rpc_backend
                    .as_ref()
                    .and_then(|backend| backend.token_auth.as_ref())
                else {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "token auth mode requires rpc_backend.token_auth configuration",
                    ));
                };
                if token_auth.issuer.trim().is_empty() || token_auth.audience.trim().is_empty() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "token auth configuration requires issuer and audience",
                    ));
                }
                if token_auth.jti_cache_ttl_ms == 0 {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "token auth jti_cache_ttl_ms must be greater than zero",
                    ));
                }
                if token_auth.shared_secret.trim().is_empty() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "token auth shared_secret must be configured",
                    ));
                }
                let _clock_skew_ms = token_auth.clock_skew_ms.unwrap_or(0);
            }
            "mtls" => {
                let Some(mtls_auth) = parsed
                    .config
                    .rpc_backend
                    .as_ref()
                    .and_then(|backend| backend.mtls_auth.as_ref())
                else {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "mtls auth mode requires rpc_backend.mtls_auth configuration",
                    ));
                };
                if mtls_auth.ca_bundle_path.trim().is_empty() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "mtls auth configuration requires ca_bundle_path",
                    ));
                }
            }
            _ => {}
        }

        let supported_capabilities = Self::sdk_supported_capabilities_for_profile(profile.as_str());
        let required_capabilities = Self::sdk_required_capabilities_for_profile(profile.as_str());
        let mut effective_capabilities = required_capabilities.clone();
        if parsed.requested_capabilities.is_empty() {
            effective_capabilities = supported_capabilities.clone();
        } else {
            let mut requested_overlap = 0_usize;
            for requested in parsed.requested_capabilities {
                let normalized = requested.trim().to_ascii_lowercase();
                if normalized.is_empty() {
                    continue;
                }
                if supported_capabilities.contains(&normalized) {
                    requested_overlap = requested_overlap.saturating_add(1);
                    if !effective_capabilities.contains(&normalized) {
                        effective_capabilities.push(normalized);
                    }
                }
            }
            if requested_overlap == 0 {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                    "no overlap between requested and supported capabilities",
                ));
            }
        }

        let limits = Self::sdk_effective_limits_for_profile(profile.as_str());

        {
            let mut guard = self
                .sdk_active_contract_version
                .lock()
                .expect("sdk_active_contract_version mutex poisoned");
            *guard = active_contract_version;
        }
        {
            let mut guard = self.sdk_profile.lock().expect("sdk_profile mutex poisoned");
            *guard = profile.clone();
        }
        {
            let mut guard = self
                .sdk_effective_capabilities
                .lock()
                .expect("sdk_effective_capabilities mutex poisoned");
            *guard = effective_capabilities.clone();
        }
        {
            let mut guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            let rpc_backend =
                parsed.config.rpc_backend.as_ref().map_or(JsonValue::Null, |backend| {
                    json!({
                        "listen_addr": backend.listen_addr,
                        "read_timeout_ms": backend.read_timeout_ms,
                        "write_timeout_ms": backend.write_timeout_ms,
                        "max_header_bytes": backend.max_header_bytes,
                        "max_body_bytes": backend.max_body_bytes,
                        "token_auth": backend.token_auth.as_ref().map(|token| json!({
                            "issuer": token.issuer,
                            "audience": token.audience,
                            "jti_cache_ttl_ms": token.jti_cache_ttl_ms,
                            "clock_skew_ms": token.clock_skew_ms.unwrap_or(0),
                            "shared_secret": token.shared_secret,
                        })),
                        "mtls_auth": backend.mtls_auth.as_ref().map(|mtls| json!({
                            "ca_bundle_path": mtls.ca_bundle_path,
                            "require_client_cert": mtls.require_client_cert,
                            "allowed_san": mtls.allowed_san,
                        })),
                    })
                });
            *guard = json!({
                "profile": profile,
                "bind_mode": bind_mode,
                "auth_mode": auth_mode,
                "overflow_policy": overflow_policy,
                "block_timeout_ms": parsed.config.block_timeout_ms,
                "rpc_backend": rpc_backend,
                "event_stream": {
                    "max_poll_events": limits.get("max_poll_events").and_then(JsonValue::as_u64).unwrap_or(256),
                    "max_event_bytes": limits.get("max_event_bytes").and_then(JsonValue::as_u64).unwrap_or(65_536),
                    "max_batch_bytes": limits.get("max_batch_bytes").and_then(JsonValue::as_u64).unwrap_or(1_048_576),
                    "max_extension_keys": limits.get("max_extension_keys").and_then(JsonValue::as_u64).unwrap_or(32),
                },
                "idempotency_ttl_ms": limits.get("idempotency_ttl_ms").and_then(JsonValue::as_u64).unwrap_or(86_400_000_u64),
                "extensions": {
                    "rate_limits": {
                        "per_ip_per_minute": 120,
                        "per_principal_per_minute": 120,
                    }
                }
            });
        }
        {
            let mut guard =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *guard = false;
        }
        {
            self.sdk_seen_jti.lock().expect("sdk_seen_jti mutex poisoned").clear();
            *self
                .sdk_rate_window_started_ms
                .lock()
                .expect("sdk_rate_window_started_ms mutex poisoned") = 0;
            self.sdk_rate_ip_counts.lock().expect("sdk_rate_ip_counts mutex poisoned").clear();
            self.sdk_rate_principal_counts
                .lock()
                .expect("sdk_rate_principal_counts mutex poisoned")
                .clear();
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "active_contract_version": active_contract_version,
                "effective_capabilities": effective_capabilities,
                "effective_limits": limits,
                "contract_release": "v2.5",
                "schema_namespace": "v2",
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_poll_events_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPollEventsV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let clear_degraded_on_success = {
            let degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            if *degraded && parsed.cursor.is_some() {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_STREAM_DEGRADED",
                    "stream is degraded; reset cursor to recover",
                ));
            }
            *degraded && parsed.cursor.is_none()
        };

        if parsed.max == 0 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "poll max must be greater than zero",
            ));
        }

        let max_poll_events = self.sdk_max_poll_events();
        if parsed.max > max_poll_events {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED",
                "poll max exceeds supported limit",
            ));
        }

        let cursor_seq = match self.sdk_decode_cursor(parsed.cursor.as_deref()) {
            Ok(value) => value,
            Err(error) => {
                return Ok(self.sdk_error_response(request.id, &error.code, &error.message))
            }
        };

        let log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
        let dropped_count =
            *self.sdk_dropped_event_count.lock().expect("sdk_dropped_event_count mutex poisoned");
        let oldest_seq = log_guard.front().map(|entry| entry.seq_no);
        let latest_seq = log_guard.back().map(|entry| entry.seq_no);

        if let (Some(cursor_seq), Some(oldest_seq)) = (cursor_seq, oldest_seq) {
            if cursor_seq.saturating_add(1) < oldest_seq {
                let mut degraded =
                    self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
                *degraded = true;
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_CURSOR_EXPIRED",
                    "cursor is outside retained event window",
                ));
            }
        }

        let start_seq = cursor_seq.map(|value| value.saturating_add(1)).or(oldest_seq).unwrap_or(0);
        let mut event_rows = Vec::new();

        if parsed.cursor.is_none() && dropped_count > 0 && event_rows.len() < parsed.max {
            let observed_seq_no = oldest_seq.unwrap_or(0);
            let expected_seq_no = observed_seq_no.saturating_sub(dropped_count);
            let gap_seq_no = observed_seq_no.saturating_sub(1);
            event_rows.push(json!({
                "event_id": format!("gap-{}", gap_seq_no),
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "seq_no": gap_seq_no,
                "contract_version": self.active_contract_version(),
                "ts_ms": (now_i64().max(0) as u64) * 1000,
                "event_type": "StreamGap",
                "severity": "warn",
                "source_component": "rns-rpc",
                "payload": {
                    "expected_seq_no": expected_seq_no,
                    "observed_seq_no": observed_seq_no,
                    "dropped_count": dropped_count,
                },
            }));
        }

        let remaining_slots = parsed.max.saturating_sub(event_rows.len());
        for entry in
            log_guard.iter().filter(|entry| entry.seq_no >= start_seq).take(remaining_slots)
        {
            event_rows.push(json!({
                "event_id": format!("evt-{}", entry.seq_no),
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "seq_no": entry.seq_no,
                "contract_version": self.active_contract_version(),
                "ts_ms": (now_i64().max(0) as u64) * 1000,
                "event_type": entry.event.event_type.clone(),
                "severity": Self::event_severity(entry.event.event_type.as_str()),
                "source_component": "rns-rpc",
                "payload": entry.event.payload.clone(),
            }));
        }

        let next_seq = event_rows
            .iter()
            .rev()
            .find_map(|event| event.get("seq_no").and_then(JsonValue::as_u64))
            .or(cursor_seq)
            .or(latest_seq)
            .unwrap_or(0);
        let next_cursor = self.sdk_encode_cursor(next_seq);

        if clear_degraded_on_success {
            let mut degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *degraded = false;
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "events": event_rows,
                "next_cursor": next_cursor,
                "dropped_count": if parsed.cursor.is_none() { dropped_count } else { 0 },
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_cancel_message_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCancelMessageV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let message_id = parsed.message_id.trim();
        if message_id.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "message_id must not be empty",
            ));
        }

        let _status_guard =
            self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
        let message = self.store.get_message(message_id).map_err(std::io::Error::other)?;
        if message.is_none() {
            return Ok(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "message_id": message_id,
                    "result": "NotFound",
                })),
                error: None,
            });
        }

        let message_status = message.and_then(|record| record.receipt_status);

        let transitions = self
            .delivery_traces
            .lock()
            .expect("delivery traces mutex poisoned")
            .get(message_id)
            .cloned()
            .unwrap_or_default();

        let mut cancel_result = "Accepted";
        if let Some(status) = &message_status {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") {
                cancel_result = "TooLateToCancel";
            } else if matches!(
                normalized.as_str(),
                "cancelled" | "delivered" | "failed" | "expired" | "rejected"
            ) {
                cancel_result = "AlreadyTerminal";
            }
        }

        for transition in &transitions {
            if cancel_result != "Accepted" {
                break;
            }
            let normalized = transition.status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") {
                cancel_result = "TooLateToCancel";
                break;
            }
            if matches!(
                normalized.as_str(),
                "cancelled" | "delivered" | "failed" | "expired" | "rejected"
            ) {
                cancel_result = "AlreadyTerminal";
                break;
            }
        }

        if cancel_result == "Accepted" {
            self.store
                .update_receipt_status(message_id, "cancelled")
                .map_err(std::io::Error::other)?;
            self.append_delivery_trace(message_id, "cancelled".to_string());
            let event = RpcEvent {
                event_type: "delivery_cancelled".into(),
                payload: json!({ "message_id": message_id, "result": "Accepted" }),
            };
            self.push_event(event.clone());
            let _ = self.events.send(event);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "message_id": message_id,
                "result": cancel_result,
            })),
            error: None,
        })
    }

    fn handle_sdk_status_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkStatusV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let message_id = parsed.message_id.trim();
        if message_id.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "message_id must not be empty",
            ));
        }
        let message = self.store.get_message(message_id).map_err(std::io::Error::other)?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "message": message,
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_configure_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkConfigureV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let patch_map = parsed.patch.as_object().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "patch must be an object")
        })?;
        const ALLOWED_KEYS: &[&str] = &[
            "overflow_policy",
            "block_timeout_ms",
            "event_stream",
            "idempotency_ttl_ms",
            "redaction",
            "rpc_backend",
            "extensions",
        ];
        if let Some(key) = patch_map.keys().find(|key| !ALLOWED_KEYS.contains(&key.as_str())) {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CONFIG_UNKNOWN_KEY",
                &format!("unknown config key '{key}'"),
            ));
        }

        let _apply_guard =
            self.sdk_config_apply_lock.lock().expect("sdk_config_apply_lock mutex poisoned");
        let mut revision_guard =
            self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        if parsed.expected_revision != *revision_guard {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CONFIG_CONFLICT",
                "config revision mismatch",
            ));
        }
        *revision_guard = revision_guard.saturating_add(1);
        let revision = *revision_guard;

        {
            let mut config_guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            merge_json_patch(&mut config_guard, &parsed.patch);
        }
        drop(revision_guard);

        let event = RpcEvent {
            event_type: "config_updated".into(),
            payload: json!({
                "revision": revision,
                "patch": parsed.patch,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "revision": revision,
            })),
            error: None,
        })
    }

    fn handle_sdk_shutdown_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkShutdownV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let mode = parsed.mode.trim().to_ascii_lowercase();
        if mode != "graceful" && mode != "immediate" {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "shutdown mode must be 'graceful' or 'immediate'",
            ));
        }

        let event = RpcEvent {
            event_type: "runtime_shutdown_requested".into(),
            payload: json!({
                "mode": mode,
                "flush_timeout_ms": parsed.flush_timeout_ms,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "mode": mode,
            })),
            error: None,
        })
    }

    fn handle_sdk_snapshot_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request
            .params
            .map(serde_json::from_value::<SdkSnapshotV2Params>)
            .transpose()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
            .unwrap_or_default();
        let active_contract_version = self.active_contract_version();
        let event_stream_position = self
            .sdk_event_log
            .lock()
            .expect("sdk_event_log mutex poisoned")
            .back()
            .map(|entry| entry.seq_no)
            .unwrap_or(0);
        let config_revision =
            *self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        let profile = self.sdk_profile.lock().expect("sdk_profile mutex poisoned").clone();
        let effective_capabilities = self
            .sdk_effective_capabilities
            .lock()
            .expect("sdk_effective_capabilities mutex poisoned")
            .clone();

        let (queued_messages, in_flight_messages) =
            self.store.count_message_buckets().map_err(std::io::Error::other)?;

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "state": "running",
                "active_contract_version": active_contract_version,
                "event_stream_position": event_stream_position,
                "config_revision": config_revision,
                "profile": profile,
                "effective_capabilities": effective_capabilities,
                "queued_messages": queued_messages,
                "in_flight_messages": in_flight_messages,
                "counts_included": params.include_counts,
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn default_sdk_identity(identity_hash: &str) -> SdkIdentityBundle {
        SdkIdentityBundle {
            identity: identity_hash.to_string(),
            public_key: format!("{identity_hash}-pub"),
            display_name: Some("default".to_string()),
            capabilities: vec!["sdk.capability.identity_hash_resolution".to_string()],
            extensions: JsonMap::new(),
        }
    }

    fn next_sdk_domain_id(&self, prefix: &str) -> String {
        let mut guard =
            self.sdk_next_domain_seq.lock().expect("sdk_next_domain_seq mutex poisoned");
        *guard = guard.saturating_add(1);
        format!("{prefix}-{:016x}", *guard)
    }

    fn sdk_has_capability(&self, capability: &str) -> bool {
        self.sdk_effective_capabilities
            .lock()
            .expect("sdk_effective_capabilities mutex poisoned")
            .iter()
            .any(|current| current == capability)
    }

    fn collection_cursor_index(
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

    fn collection_next_cursor(
        prefix: &str,
        next_index: usize,
        total_items: usize,
    ) -> Option<String> {
        if next_index >= total_items {
            return None;
        }
        Some(format!("{prefix}{next_index}"))
    }

    fn normalize_non_empty(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.to_string())
    }

    fn normalize_voice_state(value: &str) -> Option<&'static str> {
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

    fn voice_state_rank(value: &str) -> u8 {
        match value {
            "new" => 0,
            "ringing" => 1,
            "active" => 2,
            "holding" => 3,
            "closed" | "failed" => 4,
            _ => 0,
        }
    }

}
