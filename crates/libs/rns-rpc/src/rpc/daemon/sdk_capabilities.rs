impl RpcDaemon {
    fn sdk_rate_limits(&self) -> (u32, u32) {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let per_ip = config
            .get("extensions")
            .and_then(|value| value.get("rate_limits"))
            .and_then(|value| value.get("per_ip_per_minute"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(120);
        let per_principal = config
            .get("extensions")
            .and_then(|value| value.get("rate_limits"))
            .and_then(|value| value.get("per_principal_per_minute"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(120);
        (per_ip, per_principal)
    }

    fn sdk_token_auth_config(&self) -> Option<(String, String, u64, u64, String)> {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let token_auth = config.get("rpc_backend")?.get("token_auth")?;
        let issuer = token_auth.get("issuer")?.as_str()?.to_string();
        let audience = token_auth.get("audience")?.as_str()?.to_string();
        let jti_ttl_ms = token_auth.get("jti_cache_ttl_ms")?.as_u64()?;
        let clock_skew_secs =
            token_auth.get("clock_skew_ms").and_then(JsonValue::as_u64).unwrap_or(0) / 1000;
        let shared_secret = token_auth.get("shared_secret")?.as_str()?.to_string();
        Some((issuer, audience, jti_ttl_ms, clock_skew_secs, shared_secret))
    }

    fn sdk_mtls_auth_config(&self) -> Option<(bool, Option<String>)> {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let mtls_auth = config.get("rpc_backend")?.get("mtls_auth")?;
        let require_client_cert =
            mtls_auth.get("require_client_cert").and_then(JsonValue::as_bool).unwrap_or(true);
        let allowed_san = mtls_auth
            .get("allowed_san")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .and_then(|value| if value.is_empty() { None } else { Some(value.to_string()) });
        Some((require_client_cert, allowed_san))
    }

    fn header_value<'a>(headers: &'a [(String, String)], key: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }

    fn parse_token_claims(token: &str) -> Option<HashMap<String, String>> {
        let mut claims = HashMap::new();
        for part in token.split(';') {
            let (key, value) = part.split_once('=')?;
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                return None;
            }
            claims.insert(key.to_string(), value.to_string());
        }
        if claims.is_empty() {
            return None;
        }
        Some(claims)
    }

    fn token_signature(secret: &str, payload: &str) -> Option<String> {
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(payload.as_bytes());
        Some(hex::encode(mac.finalize().into_bytes()))
    }

    fn is_loopback_source(source: &str) -> bool {
        let normalized = source.trim().to_ascii_lowercase();
        normalized == "127.0.0.1"
            || normalized == "::1"
            || normalized == "[::1]"
            || normalized == "localhost"
            || normalized.starts_with("127.")
    }

    fn is_terminal_receipt_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("failed")
            || matches!(normalized.as_str(), "cancelled" | "delivered" | "expired" | "rejected")
    }

    fn active_contract_version(&self) -> u16 {
        *self
            .sdk_active_contract_version
            .lock()
            .expect("sdk_active_contract_version mutex poisoned")
    }

    fn sdk_supported_capabilities() -> Vec<String> {
        vec![
            "sdk.capability.cursor_replay".to_string(),
            "sdk.capability.async_events".to_string(),
            "sdk.capability.token_auth".to_string(),
            "sdk.capability.mtls_auth".to_string(),
            "sdk.capability.receipt_terminality".to_string(),
            "sdk.capability.config_revision_cas".to_string(),
            "sdk.capability.idempotency_ttl".to_string(),
            "sdk.capability.topics".to_string(),
            "sdk.capability.topic_subscriptions".to_string(),
            "sdk.capability.topic_fanout".to_string(),
            "sdk.capability.telemetry_query".to_string(),
            "sdk.capability.telemetry_stream".to_string(),
            "sdk.capability.attachments".to_string(),
            "sdk.capability.attachment_delete".to_string(),
            "sdk.capability.markers".to_string(),
            "sdk.capability.identity_multi".to_string(),
            "sdk.capability.identity_import_export".to_string(),
            "sdk.capability.identity_hash_resolution".to_string(),
            "sdk.capability.paper_messages".to_string(),
            "sdk.capability.remote_commands".to_string(),
            "sdk.capability.voice_signaling".to_string(),
            "sdk.capability.shared_instance_rpc_auth".to_string(),
        ]
    }

    fn sdk_supported_capabilities_for_profile(profile: &str) -> Vec<String> {
        let mut caps = Self::sdk_supported_capabilities();
        if profile == "embedded-alloc" {
            caps.retain(|capability| capability != "sdk.capability.async_events");
        }
        caps
    }

    fn sdk_required_capabilities_for_profile(profile: &str) -> Vec<String> {
        match profile {
            "desktop-local-runtime" => vec![
                "sdk.capability.cursor_replay".to_string(),
                "sdk.capability.receipt_terminality".to_string(),
                "sdk.capability.config_revision_cas".to_string(),
                "sdk.capability.idempotency_ttl".to_string(),
            ],
            _ => vec![
                "sdk.capability.cursor_replay".to_string(),
                "sdk.capability.async_events".to_string(),
                "sdk.capability.receipt_terminality".to_string(),
                "sdk.capability.config_revision_cas".to_string(),
                "sdk.capability.idempotency_ttl".to_string(),
            ],
        }
    }

    fn sdk_effective_limits_for_profile(profile: &str) -> JsonValue {
        match profile {
            "desktop-local-runtime" => json!({
                "max_poll_events": 64,
                "max_event_bytes": 32_768,
                "max_batch_bytes": 1_048_576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 43_200_000_u64,
            }),
            "embedded-alloc" => json!({
                "max_poll_events": 32,
                "max_event_bytes": 8_192,
                "max_batch_bytes": 262_144,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 7_200_000_u64,
            }),
            _ => json!({
                "max_poll_events": 256,
                "max_event_bytes": 65_536,
                "max_batch_bytes": 1_048_576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 86_400_000_u64,
            }),
        }
    }

    fn sdk_max_poll_events(&self) -> usize {
        if let Some(value) = self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("event_stream")
            .and_then(|value| value.get("max_poll_events"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| usize::try_from(value).ok())
        {
            return value;
        }
        match self.sdk_profile.lock().expect("sdk_profile mutex poisoned").as_str() {
            "desktop-local-runtime" => 64,
            "embedded-alloc" => 32,
            _ => 256,
        }
    }

    fn sdk_error_response(&self, id: u64, code: &str, message: &str) -> RpcResponse {
        RpcResponse {
            id,
            result: None,
            error: Some(RpcError { code: code.to_string(), message: message.to_string() }),
        }
    }

    fn sdk_capability_disabled_response(
        &self,
        id: u64,
        method: &str,
        capability: &str,
    ) -> RpcResponse {
        self.sdk_error_response(
            id,
            "SDK_CAPABILITY_DISABLED",
            &format!("method '{method}' requires capability '{capability}'"),
        )
    }

    fn sdk_encode_cursor(&self, seq_no: u64) -> String {
        format!("v2:{}:{}:{}", self.identity_hash, SDK_STREAM_ID, seq_no)
    }

    fn sdk_decode_cursor(&self, cursor: Option<&str>) -> Result<Option<u64>, SdkCursorError> {
        let Some(cursor) = cursor else {
            return Ok(None);
        };
        let cursor = cursor.trim();
        if cursor.is_empty() {
            return Err(SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor must not be empty".to_string(),
            });
        }

        let mut parts = cursor.split(':');
        let version = parts.next();
        let runtime_id = parts.next();
        let stream_id = parts.next();
        let seq = parts.next();
        let has_extra = parts.next().is_some();
        if version != Some("v2")
            || runtime_id != Some(self.identity_hash.as_str())
            || stream_id != Some(SDK_STREAM_ID)
            || has_extra
        {
            return Err(SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor scope does not match runtime".to_string(),
            });
        }

        let seq =
            seq.and_then(|value| value.parse::<u64>().ok()).ok_or_else(|| SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor sequence is invalid".to_string(),
            })?;
        Ok(Some(seq))
    }

    fn event_severity(event_type: &str) -> &'static str {
        if event_type.eq_ignore_ascii_case("StreamGap") {
            return "warn";
        }
        if event_type.eq_ignore_ascii_case("error")
            || event_type.eq_ignore_ascii_case("delivery_failed")
        {
            return "error";
        }
        "info"
    }
}
