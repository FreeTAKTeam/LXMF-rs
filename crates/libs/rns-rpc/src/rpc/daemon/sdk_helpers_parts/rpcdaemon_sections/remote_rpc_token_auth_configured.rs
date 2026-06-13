impl RpcDaemon {

    pub fn remote_rpc_token_auth_configured(&self) -> bool {
        let config = self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
        let bind_mode = config
            .get("bind_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("local_only")
            .trim()
            .to_ascii_lowercase();
        let auth_mode = config
            .get("auth_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("local_trusted")
            .trim()
            .to_ascii_lowercase();

        bind_mode == "remote"
            && auth_mode == "token"
            && self.validate_sdk_runtime_config(&config).is_ok()
    }

    #[allow(clippy::result_large_err)]
    pub fn configure_remote_token_auth_for_startup(
        &self,
        issuer: impl Into<String>,
        audience: impl Into<String>,
        shared_secret: impl Into<String>,
        jti_cache_ttl_ms: u64,
        clock_skew_ms: u64,
    ) -> Result<(), RpcError> {
        let profile = "desktop-full";
        let limits = Self::sdk_effective_limits_for_profile(profile);
        let store_forward = Self::default_store_forward_policy_for_profile(profile);
        let config = json!({
            "profile": profile,
            "bind_mode": "remote",
            "auth_mode": "token",
            "overflow_policy": "reject",
            "block_timeout_ms": JsonValue::Null,
            "store_forward": {
                "max_messages": store_forward.max_messages,
                "max_message_age_ms": store_forward.max_message_age_ms,
                "capacity_policy": store_forward.capacity_policy,
                "eviction_priority": store_forward.eviction_priority,
            },
            "rpc_backend": {
                "kind": "rpc",
                "listen_addr": JsonValue::Null,
                "connect_timeout_ms": 2_000,
                "request_timeout_ms": 5_000,
                "max_header_bytes": 8_192,
                "max_body_bytes": 1_048_576,
                "token_auth": {
                    "issuer": issuer.into(),
                    "audience": audience.into(),
                    "jti_cache_ttl_ms": jti_cache_ttl_ms,
                    "clock_skew_ms": clock_skew_ms,
                    "shared_secret": shared_secret.into(),
                },
                "mtls_auth": JsonValue::Null,
            },
            "event_stream": {
                "max_poll_events": limits.get("max_poll_events").and_then(JsonValue::as_u64).unwrap_or(256),
                "max_event_bytes": limits.get("max_event_bytes").and_then(JsonValue::as_u64).unwrap_or(65_536),
                "max_batch_bytes": limits.get("max_batch_bytes").and_then(JsonValue::as_u64).unwrap_or(1_048_576),
                "max_extension_keys": limits.get("max_extension_keys").and_then(JsonValue::as_u64).unwrap_or(32),
            },
            "event_sink": Self::default_event_sink_config_for_profile(profile),
            "idempotency_ttl_ms": limits.get("idempotency_ttl_ms").and_then(JsonValue::as_u64).unwrap_or(86_400_000_u64),
            "extensions": {
                "rate_limits": {
                    "per_ip_per_minute": 120,
                    "per_principal_per_minute": 120,
                }
            }
        });
        self.validate_sdk_runtime_config(&config)?;

        {
            let mut config_guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            *config_guard = config;
        }
        {
            let mut revision_guard =
                self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
            *revision_guard = revision_guard.saturating_add(1);
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
        self.persist_sdk_domain_snapshot().map_err(|err| {
            RpcError::new(
                "SDK_CONFIG_PERSIST_FAILED".to_string(),
                format!("failed to persist startup token auth config: {err}"),
            )
        })?;
        Ok(())
    }

    pub(super) fn default_store_forward_policy_for_profile(profile: &str) -> SdkStoreForwardPolicy {
        match profile {
            "embedded-alloc" => SdkStoreForwardPolicy {
                max_messages: 2_000,
                max_message_age_ms: 86_400_000,
                capacity_policy: "drop_oldest".to_string(),
                eviction_priority: "terminal_first".to_string(),
            },
            _ => SdkStoreForwardPolicy {
                max_messages: 50_000,
                max_message_age_ms: 604_800_000,
                capacity_policy: "drop_oldest".to_string(),
                eviction_priority: "terminal_first".to_string(),
            },
        }
    }

    pub(super) fn default_event_sink_config_for_profile(profile: &str) -> JsonValue {
        let max_event_bytes = match profile {
            "embedded-alloc" => 8_192_u64,
            "desktop-local-runtime" => 32_768_u64,
            _ => 65_536_u64,
        };
        json!({
            "enabled": false,
            "max_event_bytes": max_event_bytes,
            "allow_kinds": ["webhook", "mqtt", "custom"],
            "extensions": JsonMap::new(),
        })
    }

    pub(super) fn sdk_store_forward_policy(&self) -> SdkStoreForwardPolicy {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let profile = config
            .get("profile")
            .and_then(JsonValue::as_str)
            .unwrap_or("desktop-full")
            .trim()
            .to_ascii_lowercase();
        let mut policy = Self::default_store_forward_policy_for_profile(profile.as_str());
        let Some(store_forward) = config.get("store_forward").and_then(JsonValue::as_object) else {
            return policy;
        };

        if let Some(value) = store_forward.get("max_messages").and_then(JsonValue::as_u64) {
            if value > 0 && value <= STORE_FORWARD_MAX_MESSAGES_LIMIT as u64 {
                policy.max_messages = value as usize;
            }
        }
        if let Some(value) = store_forward.get("max_message_age_ms").and_then(JsonValue::as_u64) {
            if value > 0 {
                policy.max_message_age_ms = value;
            }
        }
        if let Some(value) = store_forward
            .get("capacity_policy")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .map(str::to_ascii_lowercase)
        {
            if matches!(value.as_str(), "reject_new" | "drop_oldest") {
                policy.capacity_policy = value;
            }
        }
        if let Some(value) = store_forward
            .get("eviction_priority")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .map(str::to_ascii_lowercase)
        {
            if matches!(value.as_str(), "oldest_first" | "terminal_first") {
                policy.eviction_priority = value;
            }
        }
        policy
    }

    pub(super) fn enforce_store_forward_retention(
        &self,
        now_ts: i64,
    ) -> Result<bool, std::io::Error> {
        let policy = self.sdk_store_forward_policy();
        let max_age = i64::try_from(policy.max_message_age_ms).unwrap_or(i64::MAX);
        let retention_cutoff = now_ts.saturating_sub(max_age);
        let expired_ids = self
            .store
            .expire_outbound_messages_before(retention_cutoff)
            .map_err(std::io::Error::other)?;
        if !expired_ids.is_empty() {
            for message_id in expired_ids.iter() {
                self.append_delivery_trace(message_id, "expired".to_string());
            }
            self.publish_event(RpcEvent {
                event_type: "store_forward_expired".to_string(),
                payload: json!({
                    "expired_count": expired_ids.len(),
                    "expired_ids": expired_ids,
                    "cutoff_ts_ms": retention_cutoff,
                    "max_message_age_ms": policy.max_message_age_ms,
                }),
            });
        }

        let outbound_count =
            self.store.count_outbound_messages().map_err(std::io::Error::other)? as usize;
        if outbound_count < policy.max_messages {
            return Ok(false);
        }

        if policy.capacity_policy == "reject_new" {
            self.publish_event(RpcEvent {
                event_type: "store_forward_capacity_reached".to_string(),
                payload: json!({
                    "policy": "reject_new",
                    "outbound_count": outbound_count,
                    "max_messages": policy.max_messages,
                }),
            });
            return Ok(true);
        }

        let prune_count = outbound_count.saturating_sub(policy.max_messages).saturating_add(1);
        let pruned_ids = self
            .store
            .prune_outbound_messages(prune_count, policy.eviction_priority.as_str())
            .map_err(std::io::Error::other)?;
        if !pruned_ids.is_empty() {
            for message_id in pruned_ids.iter() {
                self.append_delivery_trace(message_id, "rejected:store_forward_pruned".to_string());
            }
            self.publish_event(RpcEvent {
                event_type: "store_forward_pruned".to_string(),
                payload: json!({
                    "pruned_count": pruned_ids.len(),
                    "pruned_ids": pruned_ids,
                    "eviction_priority": policy.eviction_priority,
                    "max_messages": policy.max_messages,
                }),
            });
        }

        let remaining =
            self.store.count_outbound_messages().map_err(std::io::Error::other)? as usize;
        Ok(remaining >= policy.max_messages)
    }

    pub(super) fn default_sdk_identity(identity_hash: &str) -> SdkIdentityBundle {
        SdkIdentityBundle {
            identity: identity_hash.to_string(),
            public_key: format!("{identity_hash}-pub"),
            display_name: Some("default".to_string()),
            capabilities: vec!["sdk.capability.identity_hash_resolution".to_string()],
            extensions: JsonMap::new(),
        }
    }

    pub(super) fn next_sdk_domain_id(&self, prefix: &str) -> String {
        let mut guard =
            self.sdk_next_domain_seq.lock().expect("sdk_next_domain_seq mutex poisoned");
        *guard = guard.saturating_add(1);
        format!("{prefix}-{:016x}", *guard)
    }

    pub(super) fn sdk_has_capability(&self, capability: &str) -> bool {
        self.sdk_effective_capabilities
            .lock()
            .expect("sdk_effective_capabilities mutex poisoned")
            .iter()
            .any(|current| current == capability)
    }

    pub(super) fn should_trace_sdk_lifecycle(method: &str) -> bool {
        matches!(
            method,
            "sdk_send_v2"
                | "sdk_send_batch_v2"
                | "send_message"
                | "send_message_v2"
                | "sdk_cancel_message_v2"
                | "sdk_configure_v2"
                | "sdk_shutdown_v2"
        )
    }

    pub(super) fn sdk_lifecycle_trace_id(method: &str, request_id: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(request_id.to_le_bytes());
        hasher.update(now_millis_u64().to_le_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("sdk-trace-{}", &digest[..24])
    }

    pub(super) fn sdk_lifecycle_trace_ref(trace_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(trace_id.as_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("ref-{}", &digest[..12])
    }

    pub(super) fn sdk_lifecycle_details(
        method: &str,
        response: &RpcResponse,
    ) -> JsonMap<String, JsonValue> {
        let mut details = JsonMap::new();
        if let Some(error) = response.error.as_ref() {
            details.insert("error_code".to_string(), JsonValue::String(error.code.clone()));
        }
        let Some(result) = response.result.as_ref() else {
            return details;
        };
        match method {
            "sdk_send_v2" | "send_message" | "send_message_v2" => {
                if let Some(message_id) = result.get("message_id").and_then(JsonValue::as_str) {
                    details.insert(
                        "message_id".to_string(),
                        JsonValue::String(message_id.to_string()),
                    );
                }
            }
            "sdk_send_batch_v2" => {
                if let Some(batch_id) = result.get("batch_id").and_then(JsonValue::as_str) {
                    details.insert("batch_id".to_string(), JsonValue::String(batch_id.to_string()));
                }
                if let Some(accepted_count) =
                    result.get("accepted_count").and_then(JsonValue::as_u64)
                {
                    details.insert(
                        "accepted_count".to_string(),
                        JsonValue::Number(serde_json::Number::from(accepted_count)),
                    );
                }
                if let Some(rejected_count) =
                    result.get("rejected_count").and_then(JsonValue::as_u64)
                {
                    details.insert(
                        "rejected_count".to_string(),
                        JsonValue::Number(serde_json::Number::from(rejected_count)),
                    );
                }
            }
            "sdk_cancel_message_v2" => {
                if let Some(cancel_result) = result.get("result").and_then(JsonValue::as_str) {
                    details.insert(
                        "cancel_result".to_string(),
                        JsonValue::String(cancel_result.to_string()),
                    );
                }
            }
            "sdk_poll_events_v2" => {
                let event_count = result
                    .get("events")
                    .and_then(JsonValue::as_array)
                    .map_or(0_u64, |events| events.len() as u64);
                details.insert(
                    "event_count".to_string(),
                    JsonValue::Number(serde_json::Number::from(event_count)),
                );
                if let Some(dropped_count) = result.get("dropped_count").and_then(JsonValue::as_u64)
                {
                    details.insert(
                        "dropped_count".to_string(),
                        JsonValue::Number(serde_json::Number::from(dropped_count)),
                    );
                }
                details.insert(
                    "next_cursor_present".to_string(),
                    JsonValue::Bool(
                        result.get("next_cursor").is_some_and(|cursor| !cursor.is_null()),
                    ),
                );
            }
            "sdk_configure_v2" => {
                if let Some(revision) = result.get("revision").and_then(JsonValue::as_u64) {
                    details.insert(
                        "revision".to_string(),
                        JsonValue::Number(serde_json::Number::from(revision)),
                    );
                }
            }
            "sdk_shutdown_v2" => {
                if let Some(mode) = result.get("mode").and_then(JsonValue::as_str) {
                    details.insert("mode".to_string(), JsonValue::String(mode.to_string()));
                }
            }
            _ => {}
        }
        details
    }

    pub(super) fn emit_sdk_lifecycle_trace(
        &self,
        trace_id: &str,
        request_id: u64,
        method: &str,
        phase: &str,
        outcome: &str,
        details: JsonMap<String, JsonValue>,
    ) {
        let event = RpcEvent {
            event_type: "sdk_lifecycle_trace".to_string(),
            payload: json!({
                "trace_id": trace_id,
                "trace_ref": Self::sdk_lifecycle_trace_ref(trace_id),
                "request_id": request_id,
                "method": method,
                "phase": phase,
                "outcome": outcome,
                "timestamp_ms": now_millis_u64(),
                "details": details,
            }),
        };
        self.publish_event(event);
    }
}
