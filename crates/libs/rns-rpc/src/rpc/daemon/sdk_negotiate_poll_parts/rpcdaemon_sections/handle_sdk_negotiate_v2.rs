impl RpcDaemon {

    pub(super) fn handle_sdk_negotiate_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
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
        if !matches!(profile.as_str(), "desktop-full" | "desktop-local-runtime" | "embedded-alloc")
        {
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
        let custom_operations = match parsed.config.extensions.get("custom_operations").cloned() {
            Some(JsonValue::Null) | None => Vec::new(),
            Some(value) => match serde_json::from_value::<Vec<SdkCustomOperationSpec>>(value) {
                Ok(operations) => operations,
                Err(err) => {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        &format!("config.extensions.custom_operations is invalid: {err}"),
                    ))
                }
            },
        };

        let mut store_forward_policy =
            Self::default_store_forward_policy_for_profile(profile.as_str());
        if let Some(store_forward) = parsed.config.store_forward.as_ref() {
            if let Some(max_messages) = store_forward.max_messages {
                if max_messages == 0 || max_messages > STORE_FORWARD_MAX_MESSAGES_LIMIT {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.max_messages must be in the range 1..=1000000",
                    ));
                }
                store_forward_policy.max_messages = max_messages;
            }
            if let Some(max_message_age_ms) = store_forward.max_message_age_ms {
                if max_message_age_ms == 0 {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.max_message_age_ms must be greater than zero",
                    ));
                }
                store_forward_policy.max_message_age_ms = max_message_age_ms;
            }
            if let Some(capacity_policy) = store_forward.capacity_policy.as_deref() {
                let normalized = capacity_policy.trim().to_ascii_lowercase();
                if !matches!(normalized.as_str(), "reject_new" | "drop_oldest") {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.capacity_policy must be reject_new or drop_oldest",
                    ));
                }
                store_forward_policy.capacity_policy = normalized;
            }
            if let Some(eviction_priority) = store_forward.eviction_priority.as_deref() {
                let normalized = eviction_priority.trim().to_ascii_lowercase();
                if !matches!(normalized.as_str(), "oldest_first" | "terminal_first") {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.eviction_priority must be oldest_first or terminal_first",
                    ));
                }
                store_forward_policy.eviction_priority = normalized;
            }
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
        let mut effective_capabilities = required_capabilities;
        if !parsed.requested_capabilities.is_empty() {
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
        self.set_sdk_custom_operations(custom_operations);
        {
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
            let event_sink = parsed.config.event_sink.as_ref().map_or_else(
                || Self::default_event_sink_config_for_profile(profile.as_str()),
                |sink| {
                    let mut config = Self::default_event_sink_config_for_profile(profile.as_str());
                    if let Some(enabled) = sink.enabled {
                        config["enabled"] = json!(enabled);
                    }
                    if let Some(max_event_bytes) = sink.max_event_bytes {
                        config["max_event_bytes"] = json!(max_event_bytes);
                    }
                    if let Some(allow_kinds) = sink.allow_kinds.as_ref() {
                        config["allow_kinds"] = json!(allow_kinds);
                    }
                    if let Some(extensions) = sink.extensions.as_ref() {
                        config["extensions"] = JsonValue::Object(extensions.clone());
                    }
                    config
                },
            );
            let mut runtime_extensions = parsed.config.extensions.clone();
            runtime_extensions.insert(
                "rate_limits".to_owned(),
                json!({
                    "per_ip_per_minute": 120,
                    "per_principal_per_minute": 120,
                }),
            );
            let next_runtime_config = json!({
                "profile": profile,
                "bind_mode": bind_mode,
                "auth_mode": auth_mode,
                "overflow_policy": overflow_policy,
                "block_timeout_ms": parsed.config.block_timeout_ms,
                "store_forward": {
                    "max_messages": store_forward_policy.max_messages,
                    "max_message_age_ms": store_forward_policy.max_message_age_ms,
                    "capacity_policy": store_forward_policy.capacity_policy,
                    "eviction_priority": store_forward_policy.eviction_priority,
                },
                "rpc_backend": rpc_backend,
                "event_stream": {
                    "max_poll_events": limits.get("max_poll_events").and_then(JsonValue::as_u64).unwrap_or(256),
                    "max_event_bytes": limits.get("max_event_bytes").and_then(JsonValue::as_u64).unwrap_or(65_536),
                    "max_batch_bytes": limits.get("max_batch_bytes").and_then(JsonValue::as_u64).unwrap_or(1_048_576),
                    "max_extension_keys": limits.get("max_extension_keys").and_then(JsonValue::as_u64).unwrap_or(32),
                },
                "event_sink": event_sink,
                "idempotency_ttl_ms": limits.get("idempotency_ttl_ms").and_then(JsonValue::as_u64).unwrap_or(86_400_000_u64),
                "extensions": runtime_extensions,
            });
            if let Err(error) = self.validate_sdk_runtime_config(&next_runtime_config) {
                return Ok(RpcResponse { id: request.id, result: None, error: Some(error) });
            }
            let mut guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            *guard = next_runtime_config;
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
                "sdk_version": SDK_VERSION,
                "python_reference": python_reference_meta(),
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }
}
