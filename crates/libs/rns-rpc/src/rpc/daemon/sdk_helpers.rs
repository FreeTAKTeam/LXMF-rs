impl RpcDaemon {
    fn sdk_config_error(code: &str, message: &str) -> RpcError {
        RpcError { code: code.to_string(), message: message.to_string() }
    }

    fn validate_sdk_runtime_config(&self, config: &JsonValue) -> Result<(), RpcError> {
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
            }
            _ => {}
        }

        Ok(())
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
