impl RpcDaemon {
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
