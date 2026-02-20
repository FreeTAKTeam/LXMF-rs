use super::*;

impl RpcBackendClient {
    pub(super) fn parse_required_string(
        value: &JsonValue,
        key: &'static str,
    ) -> Result<String, SdkError> {
        value.get(key).and_then(JsonValue::as_str).map(str::to_owned).ok_or_else(|| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response missing string field '{key}'"),
            )
        })
    }

    pub(super) fn parse_required_u16(
        value: &JsonValue,
        key: &'static str,
    ) -> Result<u16, SdkError> {
        let raw = value.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response missing integer field '{key}'"),
            )
        })?;
        u16::try_from(raw).map_err(|_| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response field '{key}' is out of range"),
            )
        })
    }

    pub(super) fn parse_required_u64(
        value: &JsonValue,
        key: &'static str,
    ) -> Result<u64, SdkError> {
        value.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response missing integer field '{key}'"),
            )
        })
    }

    pub(super) fn parse_effective_limits(value: &JsonValue) -> Result<EffectiveLimits, SdkError> {
        let max_poll_events = usize::try_from(Self::parse_required_u64(value, "max_poll_events")?)
            .map_err(|_| {
                SdkError::new(code::INTERNAL, ErrorCategory::Internal, "max_poll_events overflow")
            })?;
        let max_event_bytes = usize::try_from(Self::parse_required_u64(value, "max_event_bytes")?)
            .map_err(|_| {
                SdkError::new(code::INTERNAL, ErrorCategory::Internal, "max_event_bytes overflow")
            })?;
        let max_batch_bytes = usize::try_from(Self::parse_required_u64(value, "max_batch_bytes")?)
            .map_err(|_| {
                SdkError::new(code::INTERNAL, ErrorCategory::Internal, "max_batch_bytes overflow")
            })?;
        let max_extension_keys = usize::try_from(Self::parse_required_u64(
            value,
            "max_extension_keys",
        )?)
        .map_err(|_| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, "max_extension_keys overflow")
        })?;
        let idempotency_ttl_ms = Self::parse_required_u64(value, "idempotency_ttl_ms")?;
        Ok(EffectiveLimits {
            max_poll_events,
            max_event_bytes,
            max_batch_bytes,
            max_extension_keys,
            idempotency_ttl_ms,
        })
    }

    pub(super) fn parse_severity(value: &str) -> Severity {
        if value.eq_ignore_ascii_case("debug") {
            return Severity::Debug;
        }
        if value.eq_ignore_ascii_case("warn") || value.eq_ignore_ascii_case("warning") {
            return Severity::Warn;
        }
        if value.eq_ignore_ascii_case("error") {
            return Severity::Error;
        }
        if value.eq_ignore_ascii_case("critical") || value.eq_ignore_ascii_case("fatal") {
            return Severity::Critical;
        }
        Severity::Info
    }

    pub(super) fn parse_runtime_state(value: &str) -> RuntimeState {
        if value.eq_ignore_ascii_case("new") {
            return RuntimeState::New;
        }
        if value.eq_ignore_ascii_case("starting") {
            return RuntimeState::Starting;
        }
        if value.eq_ignore_ascii_case("draining") {
            return RuntimeState::Draining;
        }
        if value.eq_ignore_ascii_case("stopped") {
            return RuntimeState::Stopped;
        }
        if value.eq_ignore_ascii_case("failed") {
            return RuntimeState::Failed;
        }
        RuntimeState::Running
    }

    pub(super) fn decode_value<T: DeserializeOwned>(
        value: JsonValue,
        context: &str,
    ) -> Result<T, SdkError> {
        serde_json::from_value(value).map_err(|err| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("failed to decode {context}: {err}"),
            )
        })
    }

    pub(super) fn decode_field_or_root<T: DeserializeOwned>(
        result: &JsonValue,
        field: &str,
        context: &str,
    ) -> Result<T, SdkError> {
        let value = result.get(field).cloned().unwrap_or_else(|| result.clone());
        Self::decode_value(value, context)
    }

    pub(super) fn decode_optional_field<T: DeserializeOwned>(
        result: &JsonValue,
        field: &str,
        context: &str,
    ) -> Result<Option<T>, SdkError> {
        let Some(value) = result.get(field) else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }
        Self::decode_value(value.clone(), context).map(Some)
    }

    pub(super) fn parse_ack(result: &JsonValue) -> Ack {
        let accepted = result
            .get("accepted")
            .and_then(JsonValue::as_bool)
            .or_else(|| {
                result.get("ack").and_then(JsonValue::as_str).map(|ack| {
                    ack.eq_ignore_ascii_case("ok") || ack.eq_ignore_ascii_case("accepted")
                })
            })
            .unwrap_or(true);
        Ack { accepted, revision: result.get("revision").and_then(JsonValue::as_u64) }
    }

    pub(super) fn parse_delivery_state(receipt_status: Option<&str>) -> DeliveryState {
        let Some(raw) = receipt_status else {
            return DeliveryState::Queued;
        };
        let normalized = raw.trim().to_ascii_lowercase();
        if normalized.starts_with("sent") {
            return DeliveryState::Sent;
        }
        if normalized.starts_with("failed") {
            return DeliveryState::Failed;
        }
        if normalized == "cancelled" {
            return DeliveryState::Cancelled;
        }
        if normalized == "delivered" {
            return DeliveryState::Delivered;
        }
        if normalized == "expired" {
            return DeliveryState::Expired;
        }
        if normalized == "rejected" {
            return DeliveryState::Rejected;
        }
        DeliveryState::InFlight
    }
}
