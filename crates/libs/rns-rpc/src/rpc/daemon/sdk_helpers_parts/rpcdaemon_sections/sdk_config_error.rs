impl RpcDaemon {

    pub(super) fn sdk_config_error(code: &str, message: &str) -> RpcError {
        RpcError::new(code, message)
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn validate_sdk_runtime_config(&self, config: &JsonValue) -> Result<(), RpcError> {
        let profile = config
            .get("profile")
            .and_then(JsonValue::as_str)
            .unwrap_or("desktop-full")
            .trim()
            .to_ascii_lowercase();
        if !matches!(profile.as_str(), "desktop-full" | "desktop-local-runtime" | "embedded-alloc")
        {
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
            const ALLOWED_STORE_FORWARD_KEYS: &[&str] =
                &["max_messages", "max_message_age_ms", "capacity_policy", "eviction_priority"];
            if let Some(key) =
                store_forward.keys().find(|key| !ALLOWED_STORE_FORWARD_KEYS.contains(&key.as_str()))
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
            const ALLOWED_EVENT_STREAM_KEYS: &[&str] =
                &["max_poll_events", "max_event_bytes", "max_batch_bytes", "max_extension_keys"];
            if let Some(key) =
                event_stream.keys().find(|key| !ALLOWED_EVENT_STREAM_KEYS.contains(&key.as_str()))
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
            if let (Some(max_event_bytes), Some(max_batch_bytes)) =
                (max_event_bytes, max_batch_bytes)
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
            if let Some(key) =
                event_sink.keys().find(|key| !ALLOWED_EVENT_SINK_KEYS.contains(&key.as_str()))
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
                let audience = token_auth.get("audience").and_then(JsonValue::as_str).unwrap_or("");
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
                if require_client_cert && (client_cert_path.is_none() || client_key_path.is_none())
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

    pub fn remote_rpc_auth_configured(&self) -> bool {
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
            && matches!(auth_mode.as_str(), "token" | "mtls")
            && self.validate_sdk_runtime_config(&config).is_ok()
    }
}
