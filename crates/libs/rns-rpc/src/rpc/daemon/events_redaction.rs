use super::*;

impl RpcDaemon {
    pub(super) fn redaction_enabled(&self) -> bool {
        self.sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("redaction")
            .and_then(|value| value.get("enabled"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(true)
    }

    pub(super) fn redaction_transform(&self) -> &'static str {
        match self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("redaction")
            .and_then(|value| value.get("sensitive_transform"))
            .and_then(JsonValue::as_str)
            .unwrap_or("hash")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "truncate" => "truncate",
            "redact" => "redact",
            _ => "hash",
        }
    }

    pub(super) fn is_sensitive_key(key: &str) -> bool {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "peer_id"
                | "destination_hash"
                | "raw_destination_hash"
                | "resolved_destination_hash"
                | "source_hash"
                | "dropped_message_id"
                | "correlation_id"
                | "trace_id"
                | "source_ip"
                | "principal"
                | "shared_secret"
                | "authorization"
                | "token"
                | "passphrase"
        )
    }

    pub(super) fn redact_scalar(value: &str, transform: &str) -> String {
        match transform {
            "truncate" => {
                let preview = value.chars().take(8).collect::<String>();
                if value.chars().count() <= 8 {
                    preview
                } else {
                    format!("{preview}...")
                }
            }
            "redact" => "[redacted]".to_string(),
            _ => {
                let mut hasher = Sha256::new();
                hasher.update(value.as_bytes());
                let digest = hex::encode(hasher.finalize());
                format!("sha256:{}", &digest[..16])
            }
        }
    }

    pub(super) fn redact_sensitive_value(value: &mut JsonValue, transform: &str) {
        let replacement = match value {
            JsonValue::String(current) => Self::redact_scalar(current, transform),
            _ => Self::redact_scalar(value.to_string().as_str(), transform),
        };
        *value = JsonValue::String(replacement);
    }

    pub(super) fn redact_json_value(value: &mut JsonValue, transform: &str) {
        match value {
            JsonValue::Object(map) => {
                for (key, inner) in map.iter_mut() {
                    if Self::is_sensitive_key(key) {
                        Self::redact_sensitive_value(inner, transform);
                    } else {
                        Self::redact_json_value(inner, transform);
                    }
                }
            }
            JsonValue::Array(items) => {
                for item in items.iter_mut() {
                    Self::redact_json_value(item, transform);
                }
            }
            _ => {}
        }
    }

    pub(super) fn redact_event(&self, mut event: RpcEvent) -> RpcEvent {
        if !self.redaction_enabled() {
            return event;
        }
        let transform = self.redaction_transform();
        Self::redact_json_value(&mut event.payload, transform);
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inbound_drop_identifier_fields_are_redacted() {
        let mut value = json!({
            "raw_destination_hash": "raw-destination",
            "resolved_destination_hash": "resolved-destination",
            "source_hash": "source",
            "destination_hash": "destination",
            "dropped_message_id": "message",
            "reason": "decode_failed",
        });

        RpcDaemon::redact_json_value(&mut value, "redact");

        assert_eq!(value["raw_destination_hash"], json!("[redacted]"));
        assert_eq!(value["resolved_destination_hash"], json!("[redacted]"));
        assert_eq!(value["source_hash"], json!("[redacted]"));
        assert_eq!(value["destination_hash"], json!("[redacted]"));
        assert_eq!(value["dropped_message_id"], json!("[redacted]"));
        assert_eq!(value["reason"], json!("decode_failed"));
    }
}
