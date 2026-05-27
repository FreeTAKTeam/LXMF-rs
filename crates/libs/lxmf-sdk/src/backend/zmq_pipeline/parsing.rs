use super::{sdk_error, ZmqPipelineBackendClient};
use crate::capability::EffectiveLimits;
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::Severity;
use crate::types::{Ack, CancelResult, DeliveryState, RuntimeState};
use serde::de::DeserializeOwned;
use serde_json::Value as JsonValue;

impl ZmqPipelineBackendClient {
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

    pub(super) fn parse_required_u16(
        value: &JsonValue,
        key: &'static str,
    ) -> Result<u16, SdkError> {
        let raw = Self::parse_required_u64(value, key)?;
        u16::try_from(raw).map_err(|_| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response field '{key}' is out of range"),
            )
        })
    }

    pub(super) fn parse_effective_limits(value: &JsonValue) -> Result<EffectiveLimits, SdkError> {
        Ok(EffectiveLimits {
            max_poll_events: usize::try_from(Self::parse_required_u64(value, "max_poll_events")?)
                .map_err(|_| {
                sdk_error(ErrorCategory::Internal, "max_poll_events overflow")
            })?,
            max_event_bytes: usize::try_from(Self::parse_required_u64(value, "max_event_bytes")?)
                .map_err(|_| {
                sdk_error(ErrorCategory::Internal, "max_event_bytes overflow")
            })?,
            max_batch_bytes: usize::try_from(Self::parse_required_u64(value, "max_batch_bytes")?)
                .map_err(|_| {
                sdk_error(ErrorCategory::Internal, "max_batch_bytes overflow")
            })?,
            max_extension_keys: usize::try_from(Self::parse_required_u64(
                value,
                "max_extension_keys",
            )?)
            .map_err(|_| sdk_error(ErrorCategory::Internal, "max_extension_keys overflow"))?,
            idempotency_ttl_ms: Self::parse_required_u64(value, "idempotency_ttl_ms")?,
        })
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

    pub(super) fn parse_cancel_result(value: &str) -> Result<CancelResult, SdkError> {
        match value {
            "Accepted" => Ok(CancelResult::Accepted),
            "AlreadyTerminal" => Ok(CancelResult::AlreadyTerminal),
            "NotFound" => Ok(CancelResult::NotFound),
            "TooLateToCancel" => Ok(CancelResult::TooLateToCancel),
            _ => Err(SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                "rpc returned unknown cancel result variant",
            )),
        }
    }

    pub(super) fn parse_delivery_state(receipt_status: Option<&str>) -> DeliveryState {
        let Some(raw) = receipt_status else {
            return DeliveryState::Queued;
        };
        let normalized = raw.trim();
        if normalized.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("sent")) {
            return DeliveryState::Sent;
        }
        if normalized.get(..6).is_some_and(|prefix| prefix.eq_ignore_ascii_case("failed")) {
            return DeliveryState::Failed;
        }
        match normalized {
            value if value.eq_ignore_ascii_case("queued") => DeliveryState::Queued,
            value if value.eq_ignore_ascii_case("dispatching") => DeliveryState::Dispatching,
            value if value.eq_ignore_ascii_case("in_flight") => DeliveryState::InFlight,
            value if value.eq_ignore_ascii_case("inflight") => DeliveryState::InFlight,
            value if value.eq_ignore_ascii_case("cancelled") => DeliveryState::Cancelled,
            value if value.eq_ignore_ascii_case("delivered") => DeliveryState::Delivered,
            value if value.eq_ignore_ascii_case("expired") => DeliveryState::Expired,
            value if value.eq_ignore_ascii_case("rejected") => DeliveryState::Rejected,
            _ => DeliveryState::Unknown,
        }
    }

    pub(super) fn parse_severity(value: &str) -> Severity {
        match value {
            raw if raw.eq_ignore_ascii_case("debug") => Severity::Debug,
            raw if raw.eq_ignore_ascii_case("info") => Severity::Info,
            raw if raw.eq_ignore_ascii_case("warn") || raw.eq_ignore_ascii_case("warning") => {
                Severity::Warn
            }
            raw if raw.eq_ignore_ascii_case("error") => Severity::Error,
            raw if raw.eq_ignore_ascii_case("critical") || raw.eq_ignore_ascii_case("fatal") => {
                Severity::Critical
            }
            _ => Severity::Unknown,
        }
    }

    pub(super) fn parse_runtime_state(value: &str) -> RuntimeState {
        match value {
            raw if raw.eq_ignore_ascii_case("new") => RuntimeState::New,
            raw if raw.eq_ignore_ascii_case("starting") => RuntimeState::Starting,
            raw if raw.eq_ignore_ascii_case("running") => RuntimeState::Running,
            raw if raw.eq_ignore_ascii_case("draining") => RuntimeState::Draining,
            raw if raw.eq_ignore_ascii_case("stopped") => RuntimeState::Stopped,
            raw if raw.eq_ignore_ascii_case("failed") => RuntimeState::Failed,
            _ => RuntimeState::Unknown,
        }
    }
}
