use crate::backend::{SdkBackend, SdkBackendAsyncEvents};
use crate::capability::{EffectiveLimits, NegotiationRequest, NegotiationResponse};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{
    EventBatch, EventCursor, EventSubscription, SdkEvent, Severity, SubscriptionStart,
};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState, MessageId, RuntimeSnapshot,
    RuntimeState, SendRequest, ShutdownMode, TickBudget, TickResult,
};
use rns_rpc::e2e_harness::{
    build_http_post, build_rpc_frame, parse_http_response_body, parse_rpc_frame,
};
use rns_rpc::RpcError;
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

pub struct RpcBackendClient {
    endpoint: String,
    next_request_id: AtomicU64,
    negotiated_capabilities: RwLock<Vec<String>>,
}

impl RpcBackendClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            next_request_id: AtomicU64::new(1),
            negotiated_capabilities: RwLock::new(Vec::new()),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn call_rpc(&self, method: &str, params: Option<JsonValue>) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let frame = build_rpc_frame(request_id, method, params).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let request = build_http_post("/rpc", &self.endpoint, &frame);
        let mut stream = TcpStream::connect(&self.endpoint).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(&request).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.shutdown(Shutdown::Write).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let body = parse_http_response_body(&response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let rpc_response = parse_rpc_frame(&body).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        if let Some(error) = rpc_response.error {
            return Err(Self::map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    fn map_rpc_error(error: RpcError) -> SdkError {
        let category = Self::map_category(error.code.as_str());
        SdkError::new(error.code, category, error.message)
    }

    fn map_category(code: &str) -> ErrorCategory {
        if code.contains("_VALIDATION_") {
            return ErrorCategory::Validation;
        }
        if code.contains("_CAPABILITY_") {
            return ErrorCategory::Capability;
        }
        if code.contains("_CONFIG_") {
            return ErrorCategory::Config;
        }
        if code.contains("_POLICY_") {
            return ErrorCategory::Policy;
        }
        if code.contains("_TRANSPORT_") {
            return ErrorCategory::Transport;
        }
        if code.contains("_STORAGE_") {
            return ErrorCategory::Storage;
        }
        if code.contains("_CRYPTO_") {
            return ErrorCategory::Crypto;
        }
        if code.contains("_TIMEOUT_") {
            return ErrorCategory::Timeout;
        }
        if code.contains("_RUNTIME_") {
            return ErrorCategory::Runtime;
        }
        if code.contains("_SECURITY_") {
            return ErrorCategory::Security;
        }
        ErrorCategory::Internal
    }

    fn profile_to_wire(profile: crate::types::Profile) -> &'static str {
        match profile {
            crate::types::Profile::DesktopFull => "desktop-full",
            crate::types::Profile::DesktopLocalRuntime => "desktop-local-runtime",
            crate::types::Profile::EmbeddedAlloc => "embedded-alloc",
        }
    }

    fn has_capability(&self, capability_id: &str) -> bool {
        self.negotiated_capabilities
            .read()
            .expect("negotiated_capabilities rwlock poisoned")
            .iter()
            .any(|capability| capability == capability_id)
    }

    fn parse_required_string(value: &JsonValue, key: &'static str) -> Result<String, SdkError> {
        value.get(key).and_then(JsonValue::as_str).map(str::to_owned).ok_or_else(|| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response missing string field '{key}'"),
            )
        })
    }

    fn parse_required_u16(value: &JsonValue, key: &'static str) -> Result<u16, SdkError> {
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

    fn parse_required_u64(value: &JsonValue, key: &'static str) -> Result<u64, SdkError> {
        value.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response missing integer field '{key}'"),
            )
        })
    }

    fn parse_effective_limits(value: &JsonValue) -> Result<EffectiveLimits, SdkError> {
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

    fn parse_severity(value: &str) -> Severity {
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

    fn parse_runtime_state(value: &str) -> RuntimeState {
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

    fn parse_delivery_state(receipt_status: Option<&str>) -> DeliveryState {
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

impl SdkBackend for RpcBackendClient {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let result = self.call_rpc(
            "sdk_negotiate_v2",
            Some(json!({
                "supported_contract_versions": req.supported_contract_versions,
                "requested_capabilities": req.requested_capabilities,
                "config": {
                    "profile": Self::profile_to_wire(req.profile),
                }
            })),
        )?;

        let runtime_id = Self::parse_required_string(&result, "runtime_id")?;
        let active_contract_version = Self::parse_required_u16(&result, "active_contract_version")?;
        let effective_capabilities = result
            .get("effective_capabilities")
            .and_then(JsonValue::as_array)
            .map(|values| {
                values.iter().filter_map(JsonValue::as_str).map(str::to_owned).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let effective_limits =
            Self::parse_effective_limits(result.get("effective_limits").ok_or_else(|| {
                SdkError::new(
                    code::INTERNAL,
                    ErrorCategory::Internal,
                    "rpc response missing effective_limits",
                )
            })?)?;
        let contract_release = Self::parse_required_string(&result, "contract_release")?;
        let schema_namespace = Self::parse_required_string(&result, "schema_namespace")?;
        {
            let mut guard = self
                .negotiated_capabilities
                .write()
                .expect("negotiated_capabilities rwlock poisoned");
            *guard = effective_capabilities.clone();
        }

        Ok(NegotiationResponse {
            runtime_id,
            active_contract_version,
            effective_capabilities,
            effective_limits,
            contract_release,
            schema_namespace,
        })
    }

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        let SendRequest {
            source,
            destination,
            payload,
            idempotency_key,
            ttl_ms,
            correlation_id,
            extensions,
        } = req;
        let rpc_message_id = format!("sdk-{}", self.next_request_id());
        let content = payload
            .get("content")
            .and_then(JsonValue::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| payload.to_string());
        let title =
            payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();

        let mut fields = match payload {
            JsonValue::Object(map) => JsonValue::Object(map),
            other => json!({ "payload": other }),
        };
        if let JsonValue::Object(map) = &mut fields {
            let mut sdk_meta = JsonMap::new();
            if let Some(idempotency_key) = idempotency_key {
                sdk_meta.insert("idempotency_key".to_string(), JsonValue::String(idempotency_key));
            }
            if let Some(ttl_ms) = ttl_ms {
                sdk_meta.insert("ttl_ms".to_string(), JsonValue::from(ttl_ms));
            }
            if let Some(correlation_id) = correlation_id {
                sdk_meta.insert("correlation_id".to_string(), JsonValue::String(correlation_id));
            }
            if !extensions.is_empty() {
                let extension_map = extensions.into_iter().collect::<JsonMap<String, JsonValue>>();
                sdk_meta.insert("extensions".to_string(), JsonValue::Object(extension_map));
            }
            if !sdk_meta.is_empty() {
                map.insert("_sdk".to_string(), JsonValue::Object(sdk_meta));
            }
        }

        let result = self.call_rpc(
            "send_message_v2",
            Some(json!({
                "id": rpc_message_id,
                "source": source,
                "destination": destination,
                "title": title,
                "content": content,
                "fields": fields,
            })),
        )?;
        let message_id = Self::parse_required_string(&result, "message_id")?;
        Ok(MessageId(message_id))
    }

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        let result = self.call_rpc(
            "sdk_cancel_message_v2",
            Some(json!({
                "message_id": id.0,
            })),
        )?;
        let value = Self::parse_required_string(&result, "result")?;
        match value.as_str() {
            "Accepted" => Ok(CancelResult::Accepted),
            "AlreadyTerminal" => Ok(CancelResult::AlreadyTerminal),
            "NotFound" => Ok(CancelResult::NotFound),
            "TooLateToCancel" => Ok(CancelResult::TooLateToCancel),
            _ => Ok(CancelResult::Unsupported),
        }
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        let message_id = id.0.clone();
        let result = self.call_rpc(
            "sdk_status_v2",
            Some(json!({
                "message_id": message_id,
            })),
        )?;
        let Some(record) = result.get("message") else {
            return Ok(None);
        };
        if record.is_null() {
            return Ok(None);
        }

        let receipt_status = record.get("receipt_status").and_then(JsonValue::as_str);
        let state = Self::parse_delivery_state(receipt_status);
        let has_receipt_terminality = self.has_capability("sdk.capability.receipt_terminality");
        let terminal = match state {
            DeliveryState::Sent => !has_receipt_terminality,
            DeliveryState::Delivered
            | DeliveryState::Failed
            | DeliveryState::Cancelled
            | DeliveryState::Expired
            | DeliveryState::Rejected => true,
            DeliveryState::Queued | DeliveryState::Dispatching | DeliveryState::InFlight => false,
        };
        let timestamp = record.get("timestamp").and_then(JsonValue::as_i64).unwrap_or(0_i64);
        let last_updated_ms = u64::try_from(timestamp.max(0)).unwrap_or(0).saturating_mul(1000);

        Ok(Some(DeliverySnapshot {
            message_id: id,
            state,
            terminal,
            last_updated_ms,
            attempts: 0,
            reason_code: None,
        }))
    }

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError> {
        let patch = serde_json::to_value(patch).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc(
            "sdk_configure_v2",
            Some(json!({
                "expected_revision": expected_revision,
                "patch": patch,
            })),
        )?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: result.get("revision").and_then(JsonValue::as_u64),
        })
    }

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError> {
        let result = self.call_rpc(
            "sdk_poll_events_v2",
            Some(json!({
                "cursor": cursor.map(|cursor| cursor.0),
                "max": max,
            })),
        )?;

        let mut events = Vec::new();
        if let Some(rows) = result.get("events").and_then(JsonValue::as_array) {
            for row in rows {
                let event_id = Self::parse_required_string(row, "event_id")?;
                let runtime_id = Self::parse_required_string(row, "runtime_id")?;
                let stream_id = Self::parse_required_string(row, "stream_id")?;
                let seq_no = Self::parse_required_u64(row, "seq_no")?;
                let contract_version = Self::parse_required_u16(row, "contract_version")?;
                let ts_ms = Self::parse_required_u64(row, "ts_ms")?;
                let event_type = Self::parse_required_string(row, "event_type")?;
                let severity = row
                    .get("severity")
                    .and_then(JsonValue::as_str)
                    .map(Self::parse_severity)
                    .unwrap_or(Severity::Info);
                let source_component = row
                    .get("source_component")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("rns-rpc")
                    .to_owned();
                let payload =
                    row.get("payload").cloned().unwrap_or(JsonValue::Object(JsonMap::new()));

                events.push(SdkEvent {
                    event_id,
                    runtime_id,
                    stream_id,
                    seq_no,
                    contract_version,
                    ts_ms,
                    event_type,
                    severity,
                    source_component,
                    operation_id: row
                        .get("operation_id")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    message_id: row
                        .get("message_id")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    peer_id: row.get("peer_id").and_then(JsonValue::as_str).map(str::to_owned),
                    correlation_id: row
                        .get("correlation_id")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    trace_id: row.get("trace_id").and_then(JsonValue::as_str).map(str::to_owned),
                    payload,
                    extensions: BTreeMap::new(),
                });
            }
        }

        let next_cursor = EventCursor(Self::parse_required_string(&result, "next_cursor")?);
        let dropped_count = result.get("dropped_count").and_then(JsonValue::as_u64).unwrap_or(0);
        let snapshot_high_watermark_seq_no =
            result.get("snapshot_high_watermark_seq_no").and_then(JsonValue::as_u64);

        Ok(EventBatch {
            events,
            next_cursor,
            dropped_count,
            snapshot_high_watermark_seq_no,
            extensions: BTreeMap::new(),
        })
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        let result = self.call_rpc("sdk_snapshot_v2", Some(json!({ "include_counts": true })))?;
        Ok(RuntimeSnapshot {
            runtime_id: Self::parse_required_string(&result, "runtime_id")?,
            state: result
                .get("state")
                .and_then(JsonValue::as_str)
                .map(Self::parse_runtime_state)
                .unwrap_or(RuntimeState::Running),
            active_contract_version: Self::parse_required_u16(&result, "active_contract_version")?,
            event_stream_position: Self::parse_required_u64(&result, "event_stream_position")?,
            config_revision: Self::parse_required_u64(&result, "config_revision")?,
            queued_messages: result.get("queued_messages").and_then(JsonValue::as_u64).unwrap_or(0),
            in_flight_messages: result
                .get("in_flight_messages")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0),
        })
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let mode = match mode {
            ShutdownMode::Graceful => "graceful",
            ShutdownMode::Immediate => "immediate",
        };
        let result = self.call_rpc(
            "sdk_shutdown_v2",
            Some(json!({
                "mode": mode,
            })),
        )?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        })
    }

    fn tick(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.manual_tick"))
    }
}

#[cfg(feature = "sdk-async")]
impl SdkBackendAsyncEvents for RpcBackendClient {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        Ok(EventSubscription { start, cursor: None })
    }
}
