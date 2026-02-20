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
        if !matches!(
            profile.as_str(),
            "desktop-full" | "desktop-local-runtime" | "embedded-alloc"
        ) {
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
        if profile == "embedded-alloc" && auth_mode == "mtls" {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "embedded-alloc profile does not support mtls auth mode",
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
                let client_cert_path = mtls_auth
                    .client_cert_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let client_key_path = mtls_auth
                    .client_key_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if client_cert_path.is_some() ^ client_key_path.is_some() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "mtls client certificate and key paths must be configured together",
                    ));
                }
                if mtls_auth.require_client_cert
                    && (client_cert_path.is_none() || client_key_path.is_none())
                {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "mtls auth configuration requires client_cert_path and client_key_path when require_client_cert=true",
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
                            "client_cert_path": mtls.client_cert_path,
                            "client_key_path": mtls.client_key_path,
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
        let max_event_bytes = self.sdk_max_event_bytes();
        let max_batch_bytes = self.sdk_max_batch_bytes();
        let max_extension_keys = self.sdk_max_extension_keys();

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

        if cursor_is_expired(cursor_seq, oldest_seq) {
            let mut degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *degraded = true;
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_CURSOR_EXPIRED",
                "cursor is outside retained event window",
            ));
        }

        let start_seq = cursor_seq.map(|value| value.saturating_add(1)).or(oldest_seq).unwrap_or(0);
        let mut event_rows = Vec::new();
        let mut batch_bytes = 0_usize;

        let append_event_row =
            |row: JsonValue, event_rows: &mut Vec<JsonValue>, batch_bytes: &mut usize| {
                let payload_bytes =
                    row.get("payload").map(|payload| payload.to_string().len()).unwrap_or(0);
                if payload_bytes > max_event_bytes {
                    return Err(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_EVENT_TOO_LARGE",
                        "event payload exceeds supported max_event_bytes limit",
                    ));
                }
                let extension_keys = row
                    .get("payload")
                    .and_then(|payload| payload.get("extensions"))
                    .and_then(JsonValue::as_object)
                    .map_or(0, JsonMap::len);
                if extension_keys > max_extension_keys {
                    return Err(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED",
                        "event extensions key count exceeds supported limit",
                    ));
                }
                let event_bytes = row.to_string().len();
                let next_batch_bytes = (*batch_bytes).saturating_add(event_bytes);
                if next_batch_bytes > max_batch_bytes {
                    return Err(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_BATCH_TOO_LARGE",
                        "event batch exceeds supported max_batch_bytes limit",
                    ));
                }
                *batch_bytes = next_batch_bytes;
                event_rows.push(row);
                Ok(())
            };

        if parsed.cursor.is_none() && event_rows.len() < parsed.max {
            if let Some(gap_meta) = compute_stream_gap(dropped_count, oldest_seq) {
            let gap_row = json!({
                    "event_id": format!("gap-{}", gap_meta.gap_seq_no),
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                    "seq_no": gap_meta.gap_seq_no,
                "contract_version": self.active_contract_version(),
                "ts_ms": (now_i64().max(0) as u64) * 1000,
                "event_type": "StreamGap",
                "severity": "warn",
                "source_component": "rns-rpc",
                "payload": {
                        "expected_seq_no": gap_meta.expected_seq_no,
                        "observed_seq_no": gap_meta.observed_seq_no,
                        "dropped_count": gap_meta.dropped_count,
                },
            });
                if let Err(response) = append_event_row(gap_row, &mut event_rows, &mut batch_bytes)
                {
                    return Ok(response);
                }
            }
        }

        let remaining_slots = parsed.max.saturating_sub(event_rows.len());
        for entry in log_guard
            .iter()
            .filter(|entry| entry.seq_no >= start_seq)
            .filter(|entry| entry.event.event_type != "sdk_lifecycle_trace")
            .take(remaining_slots)
        {
            let event_row = json!({
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
            });
            if let Err(response) = append_event_row(event_row, &mut event_rows, &mut batch_bytes) {
                return Ok(response);
            }
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

}
