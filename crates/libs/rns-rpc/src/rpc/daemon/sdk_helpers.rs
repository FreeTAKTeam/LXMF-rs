const STORE_FORWARD_MAX_MESSAGES_LIMIT: usize = 1_000_000;
const EVENT_SINK_MAX_EVENT_BYTES_LIMIT: u64 = 2_097_152;

#[derive(Clone, Debug)]
struct SdkStoreForwardPolicy {
    max_messages: usize,
    max_message_age_ms: u64,
    capacity_policy: String,
    eviction_priority: String,
}

impl RpcDaemon {
    fn sdk_config_error(code: &str, message: &str) -> RpcError {
        RpcError::new(code, message)
    }

    #[allow(clippy::result_large_err)]
    fn validate_sdk_runtime_config(&self, config: &JsonValue) -> Result<(), RpcError> {
        let profile = config
            .get("profile")
            .and_then(JsonValue::as_str)
            .unwrap_or("desktop-full")
            .trim()
            .to_ascii_lowercase();
        if !matches!(
            profile.as_str(),
            "desktop-full" | "desktop-local-runtime" | "embedded-alloc"
        ) {
            return Err(Self::sdk_config_error(
                "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                "profile is not supported by the rpc backend",
            ));
        }

        let bind_mode = config
            .get("bind_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("local_only")
            .trim()
            .to_ascii_lowercase();
        if !matches!(bind_mode.as_str(), "local_only" | "remote") {
            return Err(Self::sdk_config_error(
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "bind_mode must be local_only or remote",
            ));
        }

        let auth_mode = config
            .get("auth_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("local_trusted")
            .trim()
            .to_ascii_lowercase();
        if !matches!(auth_mode.as_str(), "local_trusted" | "token" | "mtls") {
            return Err(Self::sdk_config_error(
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "auth_mode must be local_trusted, token, or mtls",
            ));
        }
        if bind_mode == "remote" && !matches!(auth_mode.as_str(), "token" | "mtls") {
            return Err(Self::sdk_config_error(
                "SDK_SECURITY_REMOTE_BIND_DISALLOWED",
                "remote bind mode requires token or mtls auth mode",
            ));
        }
        if bind_mode == "local_only" && auth_mode != "local_trusted" {
            return Err(Self::sdk_config_error(
                "SDK_SECURITY_AUTH_REQUIRED",
                "local_only bind mode requires local_trusted auth mode",
            ));
        }
        if profile == "embedded-alloc" && auth_mode == "mtls" {
            return Err(Self::sdk_config_error(
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "embedded-alloc profile does not support mtls auth mode",
            ));
        }

        let overflow_policy = config
            .get("overflow_policy")
            .and_then(JsonValue::as_str)
            .unwrap_or("reject")
            .trim()
            .to_ascii_lowercase();
        if !matches!(overflow_policy.as_str(), "reject" | "drop_oldest" | "block") {
            return Err(Self::sdk_config_error(
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "overflow_policy must be reject, drop_oldest, or block",
            ));
        }
        if overflow_policy == "block"
            && config.get("block_timeout_ms").and_then(JsonValue::as_u64).is_none()
        {
            return Err(Self::sdk_config_error(
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "overflow_policy=block requires block_timeout_ms",
            ));
        }

        if let Some(store_forward) = config.get("store_forward") {
            if !store_forward.is_object() && !store_forward.is_null() {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "store_forward must be an object when provided",
                ));
            }
        }
        if let Some(store_forward) = config.get("store_forward").and_then(JsonValue::as_object) {
            const ALLOWED_STORE_FORWARD_KEYS: &[&str] = &[
                "max_messages",
                "max_message_age_ms",
                "capacity_policy",
                "eviction_priority",
            ];
            if let Some(key) = store_forward
                .keys()
                .find(|key| !ALLOWED_STORE_FORWARD_KEYS.contains(&key.as_str()))
            {
                return Err(Self::sdk_config_error(
                    "SDK_CONFIG_UNKNOWN_KEY",
                    &format!("unknown store_forward key '{key}'"),
                ));
            }
            if let Some(max_messages) = store_forward.get("max_messages") {
                let Some(value) = max_messages.as_u64() else {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.max_messages must be an unsigned integer",
                    ));
                };
                if value == 0 || value > STORE_FORWARD_MAX_MESSAGES_LIMIT as u64 {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.max_messages must be in the range 1..=1000000",
                    ));
                }
            }
            if let Some(max_message_age_ms) = store_forward.get("max_message_age_ms") {
                let Some(value) = max_message_age_ms.as_u64() else {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.max_message_age_ms must be an unsigned integer",
                    ));
                };
                if value == 0 {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.max_message_age_ms must be greater than zero",
                    ));
                }
            }
            if let Some(capacity_policy) = store_forward
                .get("capacity_policy")
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .map(str::to_ascii_lowercase)
            {
                if !matches!(capacity_policy.as_str(), "reject_new" | "drop_oldest") {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.capacity_policy must be reject_new or drop_oldest",
                    ));
                }
            }
            if let Some(eviction_priority) = store_forward
                .get("eviction_priority")
                .and_then(JsonValue::as_str)
                .map(str::trim)
                .map(str::to_ascii_lowercase)
            {
                if !matches!(eviction_priority.as_str(), "oldest_first" | "terminal_first") {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "store_forward.eviction_priority must be oldest_first or terminal_first",
                    ));
                }
            }
        }

        if let Some(event_stream) = config.get("event_stream") {
            if !event_stream.is_object() && !event_stream.is_null() {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "event_stream must be an object when provided",
                ));
            }
        }
        if let Some(event_stream) = config.get("event_stream").and_then(JsonValue::as_object) {
            const ALLOWED_EVENT_STREAM_KEYS: &[&str] = &[
                "max_poll_events",
                "max_event_bytes",
                "max_batch_bytes",
                "max_extension_keys",
            ];
            if let Some(key) = event_stream
                .keys()
                .find(|key| !ALLOWED_EVENT_STREAM_KEYS.contains(&key.as_str()))
            {
                return Err(Self::sdk_config_error(
                    "SDK_CONFIG_UNKNOWN_KEY",
                    &format!("unknown event_stream key '{key}'"),
                ));
            }

            let parse_u64_field = |key: &str| -> Result<Option<u64>, RpcError> {
                match event_stream.get(key) {
                    None | Some(JsonValue::Null) => Ok(None),
                    Some(value) => value.as_u64().map(Some).ok_or_else(|| {
                        Self::sdk_config_error(
                            "SDK_VALIDATION_INVALID_ARGUMENT",
                            &format!("event_stream.{key} must be an unsigned integer"),
                        )
                    }),
                }
            };
            let max_poll_events = parse_u64_field("max_poll_events")?;
            let max_event_bytes = parse_u64_field("max_event_bytes")?;
            let max_batch_bytes = parse_u64_field("max_batch_bytes")?;
            let max_extension_keys = parse_u64_field("max_extension_keys")?;

            if max_poll_events.is_some_and(|value| value == 0 || value > 10_000) {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "event_stream.max_poll_events must be in the range 1..=10000",
                ));
            }
            if max_event_bytes.is_some_and(|value| value < 256) {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "event_stream.max_event_bytes must be at least 256",
                ));
            }
            if max_batch_bytes.is_some_and(|value| value < 1_024) {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "event_stream.max_batch_bytes must be at least 1024",
                ));
            }
            if max_extension_keys.is_some_and(|value| value > 32) {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "event_stream.max_extension_keys must be in the range 0..=32",
                ));
            }
            if let (Some(max_event_bytes), Some(max_batch_bytes)) = (max_event_bytes, max_batch_bytes)
            {
                if max_batch_bytes < max_event_bytes {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "event_stream.max_batch_bytes must be greater than or equal to max_event_bytes",
                    ));
                }
            }
        }

        if let Some(event_sink) = config.get("event_sink") {
            if !event_sink.is_object() && !event_sink.is_null() {
                return Err(Self::sdk_config_error(
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "event_sink must be an object when provided",
                ));
            }
        }
        if let Some(event_sink) = config.get("event_sink").and_then(JsonValue::as_object) {
            const ALLOWED_EVENT_SINK_KEYS: &[&str] =
                &["enabled", "max_event_bytes", "allow_kinds", "extensions"];
            if let Some(key) = event_sink
                .keys()
                .find(|key| !ALLOWED_EVENT_SINK_KEYS.contains(&key.as_str()))
            {
                return Err(Self::sdk_config_error(
                    "SDK_CONFIG_UNKNOWN_KEY",
                    &format!("unknown event_sink key '{key}'"),
                ));
            }
            if let Some(enabled) = event_sink.get("enabled") {
                if !enabled.is_boolean() {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "event_sink.enabled must be a boolean",
                    ));
                }
            }
            if let Some(max_event_bytes) = event_sink.get("max_event_bytes") {
                let Some(value) = max_event_bytes.as_u64() else {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "event_sink.max_event_bytes must be an unsigned integer",
                    ));
                };
                if !(256..=EVENT_SINK_MAX_EVENT_BYTES_LIMIT).contains(&value) {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "event_sink.max_event_bytes must be in the range 256..=2097152",
                    ));
                }
            }
            if let Some(allow_kinds) = event_sink.get("allow_kinds") {
                let Some(values) = allow_kinds.as_array() else {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "event_sink.allow_kinds must be an array of strings",
                    ));
                };
                if values.is_empty() {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "event_sink.allow_kinds must include at least one sink kind",
                    ));
                }
                for value in values {
                    let Some(kind) = value
                        .as_str()
                        .map(str::trim)
                        .map(str::to_ascii_lowercase)
                        .filter(|kind| !kind.is_empty())
                    else {
                        return Err(Self::sdk_config_error(
                            "SDK_VALIDATION_INVALID_ARGUMENT",
                            "event_sink.allow_kinds entries must be non-empty strings",
                        ));
                    };
                    if !matches!(kind.as_str(), "webhook" | "mqtt" | "custom") {
                        return Err(Self::sdk_config_error(
                            "SDK_VALIDATION_INVALID_ARGUMENT",
                            "event_sink.allow_kinds supports webhook, mqtt, or custom",
                        ));
                    }
                }
            }
            if event_sink.get("enabled").and_then(JsonValue::as_bool).unwrap_or(false)
                && !config
                    .get("redaction")
                    .and_then(|value| value.get("enabled"))
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(true)
            {
                return Err(Self::sdk_config_error(
                    "SDK_SECURITY_REDACTION_REQUIRED",
                    "event_sink.enabled requires redaction.enabled=true",
                ));
            }
        }

        match auth_mode.as_str() {
            "token" => {
                let Some(token_auth) = config
                    .get("rpc_backend")
                    .and_then(|value| value.get("token_auth"))
                    .and_then(JsonValue::as_object)
                else {
                    return Err(Self::sdk_config_error(
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "token auth mode requires rpc_backend.token_auth configuration",
                    ));
                };
                let issuer = token_auth.get("issuer").and_then(JsonValue::as_str).unwrap_or("");
                let audience =
                    token_auth.get("audience").and_then(JsonValue::as_str).unwrap_or("");
                if issuer.trim().is_empty() || audience.trim().is_empty() {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "token auth configuration requires issuer and audience",
                    ));
                }
                let jti_cache_ttl_ms =
                    token_auth.get("jti_cache_ttl_ms").and_then(JsonValue::as_u64).unwrap_or(0);
                if jti_cache_ttl_ms == 0 {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "token auth jti_cache_ttl_ms must be greater than zero",
                    ));
                }
                let shared_secret =
                    token_auth.get("shared_secret").and_then(JsonValue::as_str).unwrap_or("");
                if shared_secret.trim().is_empty() {
                    return Err(Self::sdk_config_error(
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "token auth shared_secret must be configured",
                    ));
                }
            }
            "mtls" => {
                let Some(mtls_auth) = config
                    .get("rpc_backend")
                    .and_then(|value| value.get("mtls_auth"))
                    .and_then(JsonValue::as_object)
                else {
                    return Err(Self::sdk_config_error(
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "mtls auth mode requires rpc_backend.mtls_auth configuration",
                    ));
                };
                let ca_bundle_path =
                    mtls_auth.get("ca_bundle_path").and_then(JsonValue::as_str).unwrap_or("");
                if ca_bundle_path.trim().is_empty() {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "mtls auth configuration requires ca_bundle_path",
                    ));
                }
                let client_cert_path = mtls_auth
                    .get("client_cert_path")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let client_key_path = mtls_auth
                    .get("client_key_path")
                    .and_then(JsonValue::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if client_cert_path.is_some() ^ client_key_path.is_some() {
                    return Err(Self::sdk_config_error(
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "mtls client certificate and key paths must be configured together",
                    ));
                }
                let require_client_cert = mtls_auth
                    .get("require_client_cert")
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(true);
                if require_client_cert
                    && (client_cert_path.is_none() || client_key_path.is_none())
                {
                    return Err(Self::sdk_config_error(
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "mtls auth configuration requires client_cert_path and client_key_path when require_client_cert=true",
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn default_store_forward_policy_for_profile(profile: &str) -> SdkStoreForwardPolicy {
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

    fn default_event_sink_config_for_profile(profile: &str) -> JsonValue {
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

    fn sdk_store_forward_policy(&self) -> SdkStoreForwardPolicy {
        let config = self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .clone();
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

    fn enforce_store_forward_retention(&self, now_ts: i64) -> Result<bool, std::io::Error> {
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

        let prune_count = outbound_count
            .saturating_sub(policy.max_messages)
            .saturating_add(1);
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

    fn should_trace_sdk_lifecycle(method: &str) -> bool {
        matches!(
            method,
            "sdk_send_v2"
                | "send_message"
                | "send_message_v2"
                | "sdk_cancel_message_v2"
                | "sdk_configure_v2"
                | "sdk_shutdown_v2"
        )
    }

    fn sdk_lifecycle_trace_id(method: &str, request_id: u64) -> String {
        let mut hasher = Sha256::new();
        hasher.update(method.as_bytes());
        hasher.update(request_id.to_le_bytes());
        hasher.update(now_millis_u64().to_le_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("sdk-trace-{}", &digest[..24])
    }

    fn sdk_lifecycle_trace_ref(trace_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(trace_id.as_bytes());
        let digest = hex::encode(hasher.finalize());
        format!("ref-{}", &digest[..12])
    }

    fn sdk_lifecycle_details(method: &str, response: &RpcResponse) -> JsonMap<String, JsonValue> {
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

    fn emit_sdk_lifecycle_trace(
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

    fn parse_domain_sequence(id: &str) -> Option<u64> {
        let (_, suffix) = id.rsplit_once('-')?;
        if suffix.len() != 16 {
            return None;
        }
        u64::from_str_radix(suffix, 16).ok()
    }

    fn infer_snapshot_domain_sequence(snapshot: &SdkDomainSnapshotV1) -> u64 {
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
        for id in snapshot.remote_commands.iter() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        for id in snapshot.voice_sessions.keys() {
            max_seq = max_seq.max(Self::parse_domain_sequence(id).unwrap_or(0));
        }
        max_seq
    }

    fn default_identity_map(&self) -> HashMap<String, SdkIdentityBundle> {
        let mut identities = HashMap::new();
        identities.insert(
            self.identity_hash.clone(),
            Self::default_sdk_identity(self.identity_hash.as_str()),
        );
        identities
    }

    fn build_sdk_domain_snapshot(&self) -> SdkDomainSnapshotV1 {
        let next_domain_seq =
            *self.sdk_next_domain_seq.lock().expect("sdk_next_domain_seq mutex poisoned");
        let config_revision = *self
            .sdk_config_revision
            .lock()
            .expect("sdk_config_revision mutex poisoned");
        let runtime_config = self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .clone();
        let topics = self.sdk_topics.lock().expect("sdk_topics mutex poisoned").clone();
        let topic_order =
            self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned").clone();
        let topic_subscriptions = self
            .sdk_topic_subscriptions
            .lock()
            .expect("sdk_topic_subscriptions mutex poisoned")
            .clone();
        let telemetry_points = self
            .sdk_telemetry_points
            .lock()
            .expect("sdk_telemetry_points mutex poisoned")
            .clone();
        let attachments =
            self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned").clone();
        let attachment_payloads = self
            .sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned")
            .clone();
        let attachment_order = self
            .sdk_attachment_order
            .lock()
            .expect("sdk_attachment_order mutex poisoned")
            .clone();
        let markers = self.sdk_markers.lock().expect("sdk_markers mutex poisoned").clone();
        let marker_order =
            self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned").clone();
        let identities =
            self.sdk_identities.lock().expect("sdk_identities mutex poisoned").clone();
        let contacts =
            self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned").clone();
        let contact_order =
            self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned").clone();
        let active_identity = self
            .sdk_active_identity
            .lock()
            .expect("sdk_active_identity mutex poisoned")
            .clone();
        let remote_commands = self
            .sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned")
            .clone();
        let voice_sessions = self
            .sdk_voice_sessions
            .lock()
            .expect("sdk_voice_sessions mutex poisoned")
            .clone();

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

    fn normalize_sdk_domain_snapshot(
        &self,
        mut snapshot: SdkDomainSnapshotV1,
    ) -> SdkDomainSnapshotV1 {
        snapshot.topic_order.retain(|topic_id| snapshot.topics.contains_key(topic_id));
        snapshot
            .topic_subscriptions
            .retain(|topic_id| snapshot.topics.contains_key(topic_id));
        snapshot
            .attachment_order
            .retain(|attachment_id| snapshot.attachments.contains_key(attachment_id));
        snapshot.marker_order.retain(|marker_id| snapshot.markers.contains_key(marker_id));
        snapshot
            .contact_order
            .retain(|identity| snapshot.contacts.contains_key(identity));
        snapshot.attachment_payloads.retain(|attachment_id, _| {
            snapshot.attachments.contains_key(attachment_id)
        });

        if snapshot.identities.is_empty() {
            snapshot.identities = self.default_identity_map();
        }
        snapshot
            .identities
            .entry(self.identity_hash.clone())
            .or_insert_with(|| Self::default_sdk_identity(self.identity_hash.as_str()));
        let active_identity_valid = snapshot.active_identity.as_ref().is_some_and(|value| {
            snapshot.identities.contains_key(value)
        });
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

    fn restore_sdk_domain_snapshot(&self) -> Result<(), std::io::Error> {
        let snapshot = self
            .store
            .get_sdk_domain_snapshot()
            .map_err(std::io::Error::other)?;
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
        *self
            .sdk_config_revision
            .lock()
            .expect("sdk_config_revision mutex poisoned") = config_revision;
        *self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned") = runtime_config;
        *self.sdk_topics.lock().expect("sdk_topics mutex poisoned") = parsed.topics;
        *self.sdk_topic_order.lock().expect("sdk_topic_order mutex poisoned") = parsed.topic_order;
        *self
            .sdk_topic_subscriptions
            .lock()
            .expect("sdk_topic_subscriptions mutex poisoned") = parsed.topic_subscriptions;
        *self
            .sdk_telemetry_points
            .lock()
            .expect("sdk_telemetry_points mutex poisoned") = parsed.telemetry_points;
        *self.sdk_attachments.lock().expect("sdk_attachments mutex poisoned") = parsed.attachments;
        *self
            .sdk_attachment_payloads
            .lock()
            .expect("sdk_attachment_payloads mutex poisoned") = parsed.attachment_payloads;
        *self
            .sdk_attachment_order
            .lock()
            .expect("sdk_attachment_order mutex poisoned") = parsed.attachment_order;
        *self.sdk_markers.lock().expect("sdk_markers mutex poisoned") = parsed.markers;
        *self.sdk_marker_order.lock().expect("sdk_marker_order mutex poisoned") = parsed.marker_order;
        *self.sdk_identities.lock().expect("sdk_identities mutex poisoned") = parsed.identities;
        *self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned") = parsed.contacts;
        *self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned") = parsed.contact_order;
        *self
            .sdk_active_identity
            .lock()
            .expect("sdk_active_identity mutex poisoned") = parsed.active_identity;
        *self
            .sdk_remote_commands
            .lock()
            .expect("sdk_remote_commands mutex poisoned") = parsed.remote_commands;
        *self
            .sdk_voice_sessions
            .lock()
            .expect("sdk_voice_sessions mutex poisoned") = parsed.voice_sessions;
        Ok(())
    }

    fn lock_and_restore_sdk_domain_snapshot(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, ()>, std::io::Error> {
        let guard = self
            .sdk_domain_state_lock
            .lock()
            .expect("sdk_domain_state_lock mutex poisoned");
        self.restore_sdk_domain_snapshot()?;
        Ok(guard)
    }

    fn persist_sdk_domain_snapshot(&self) -> Result<(), std::io::Error> {
        let snapshot = self.build_sdk_domain_snapshot();
        let value = serde_json::to_value(&snapshot).map_err(std::io::Error::other)?;
        self.store
            .put_sdk_domain_snapshot(&value)
            .map_err(std::io::Error::other)?;
        Ok(())
    }

}
