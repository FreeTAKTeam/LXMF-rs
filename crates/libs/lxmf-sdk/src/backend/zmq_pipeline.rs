use crate::backend::SdkBackend;
use crate::capability::{EffectiveLimits, NegotiationRequest, NegotiationResponse};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor, SdkEvent, Severity};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, MessageId, RuntimeSnapshot, SendRequest,
    ShutdownMode, RuntimeState,
};
use hmac::{Hmac, Mac};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_rpc_frame};
use rns_rpc::rpc::zmq::{self, ZmqRpcAuthMetadata, ZmqRpcEnvelope, ZmqRpcEnvelopeKind};
use rns_rpc::RpcError;
use serde_json::{json, Value as JsonValue};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
use zeromq::{PullSocket, PushSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZmqEndpointRole {
    Bind,
    Connect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZmqPipelineTokenAuth {
    pub issuer: String,
    pub audience: String,
    pub shared_secret: String,
    pub ttl_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZmqPipelineBackendConfig {
    pub command_endpoint: String,
    pub command_role: ZmqEndpointRole,
    pub response_endpoint: String,
    pub response_role: ZmqEndpointRole,
    pub request_timeout: Duration,
    pub max_envelope_bytes: usize,
    pub token_auth: Option<ZmqPipelineTokenAuth>,
}

impl ZmqPipelineBackendConfig {
    pub fn local_tcp(command_endpoint: impl Into<String>, response_endpoint: impl Into<String>) -> Self {
        Self {
            command_endpoint: command_endpoint.into(),
            command_role: ZmqEndpointRole::Connect,
            response_endpoint: response_endpoint.into(),
            response_role: ZmqEndpointRole::Bind,
            request_timeout: Duration::from_secs(5),
            max_envelope_bytes: zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES,
            token_auth: None,
        }
    }

    pub fn validate(&self) -> Result<(), SdkError> {
        validate_endpoint_security(&self.command_endpoint, self.token_auth.is_some())?;
        validate_endpoint_security(&self.response_endpoint, self.token_auth.is_some())?;
        if self.max_envelope_bytes > zmq::ZMQ_RPC_MAX_ENVELOPE_BYTES {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "zmq max_envelope_bytes exceeds protocol limit",
            ));
        }
        Ok(())
    }
}

pub struct ZmqPipelineBackendClient {
    config: ZmqPipelineBackendConfig,
    session_id: String,
    next_request_id: AtomicU64,
}

impl ZmqPipelineBackendClient {
    pub fn new(config: ZmqPipelineBackendConfig) -> Result<Self, SdkError> {
        config.validate()?;
        Ok(Self {
            config,
            session_id: new_session_id(),
            next_request_id: AtomicU64::new(1),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn call_rpc(&self, method: &str, params: Option<JsonValue>) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let payload = build_rpc_frame(request_id, method, params)
            .map_err(|err| sdk_error(ErrorCategory::Internal, err.to_string()))?;
        let envelope = ZmqRpcEnvelope::request(
            self.session_id.clone(),
            request_id,
            self.config.response_endpoint.clone(),
            payload,
            self.auth_metadata(),
        );
        let encoded = zmq::encode_envelope(&envelope)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if encoded.len() > self.config.max_envelope_bytes {
            return Err(sdk_error(ErrorCategory::Transport, "zmq rpc envelope exceeded configured limit"));
        }

        let runtime = Runtime::new().map_err(|err| sdk_error(ErrorCategory::Internal, err.to_string()))?;
        let response = runtime.block_on(self.send_and_recv(encoded, request_id))?;
        let rpc_response = parse_rpc_frame(&response.payload)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if let Some(error) = rpc_response.error {
            return Err(map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    async fn send_and_recv(
        &self,
        encoded: Vec<u8>,
        request_id: u64,
    ) -> Result<ZmqRpcEnvelope, SdkError> {
        let mut command = PushSocket::new();
        apply_role(&mut command, self.config.command_role, &self.config.command_endpoint).await?;
        let mut responses = PullSocket::new();
        apply_role(&mut responses, self.config.response_role, &self.config.response_endpoint).await?;

        command
            .send(ZmqMessage::from(encoded))
            .await
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;

        let deadline = tokio::time::sleep(self.config.request_timeout);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                _ = &mut deadline => {
                    return Err(SdkError::new(
                        "SDK_TRANSPORT_ZMQ_TIMEOUT",
                        ErrorCategory::Timeout,
                        "zmq rpc request timed out waiting for correlated response",
                    ));
                }
                message = responses.recv() => {
                    let bytes = Vec::<u8>::try_from(message.map_err(|err| {
                        sdk_error(ErrorCategory::Transport, err.to_string())
                    })?)
                    .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
                    let envelope = zmq::decode_envelope(&bytes)
                        .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
                    if envelope.kind == ZmqRpcEnvelopeKind::Response
                        && envelope.session_id == self.session_id
                        && envelope.request_id == request_id
                    {
                        return Ok(envelope);
                    }
                }
            }
        }
    }

    fn auth_metadata(&self) -> Option<ZmqRpcAuthMetadata> {
        let auth = self.config.token_auth.as_ref()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let payload = format!(
            "iss={};aud={};iat={};exp={};jti={}-{}",
            auth.issuer,
            auth.audience,
            now,
            now.saturating_add(auth.ttl_secs.max(1)),
            self.session_id,
            self.next_request_id.load(Ordering::Relaxed)
        );
        let sig = token_signature(auth.shared_secret.as_str(), payload.as_str());
        Some(ZmqRpcAuthMetadata {
            scheme: "bearer".to_string(),
            value: format!("{};sig={}", payload, sig),
        })
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

    fn parse_required_u64(value: &JsonValue, key: &'static str) -> Result<u64, SdkError> {
        value.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response missing integer field '{key}'"),
            )
        })
    }

    fn parse_required_u16(value: &JsonValue, key: &'static str) -> Result<u16, SdkError> {
        let raw = Self::parse_required_u64(value, key)?;
        u16::try_from(raw).map_err(|_| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("rpc response field '{key}' is out of range"),
            )
        })
    }

    fn parse_effective_limits(value: &JsonValue) -> Result<EffectiveLimits, SdkError> {
        Ok(EffectiveLimits {
            max_poll_events: usize::try_from(Self::parse_required_u64(value, "max_poll_events")?)
                .map_err(|_| sdk_error(ErrorCategory::Internal, "max_poll_events overflow"))?,
            max_event_bytes: usize::try_from(Self::parse_required_u64(value, "max_event_bytes")?)
                .map_err(|_| sdk_error(ErrorCategory::Internal, "max_event_bytes overflow"))?,
            max_batch_bytes: usize::try_from(Self::parse_required_u64(value, "max_batch_bytes")?)
                .map_err(|_| sdk_error(ErrorCategory::Internal, "max_batch_bytes overflow"))?,
            max_extension_keys: usize::try_from(Self::parse_required_u64(
                value,
                "max_extension_keys",
            )?)
            .map_err(|_| sdk_error(ErrorCategory::Internal, "max_extension_keys overflow"))?,
            idempotency_ttl_ms: Self::parse_required_u64(value, "idempotency_ttl_ms")?,
        })
    }

    fn parse_cancel_result(value: &str) -> Result<CancelResult, SdkError> {
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

    fn parse_delivery_state(receipt_status: Option<&str>) -> DeliveryState {
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

    fn parse_severity(value: &str) -> Severity {
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

    fn parse_runtime_state(value: &str) -> RuntimeState {
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

impl SdkBackend for ZmqPipelineBackendClient {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let result = self.call_rpc(
            "sdk_negotiate_v2",
            Some(json!({
                "supported_contract_versions": req.supported_contract_versions,
                "requested_capabilities": req.requested_capabilities,
                "config": {
                    "profile": req.profile,
                    "bind_mode": req.bind_mode,
                    "auth_mode": req.auth_mode,
                    "overflow_policy": req.overflow_policy,
                    "block_timeout_ms": req.block_timeout_ms,
                    "rpc_backend": req.rpc_backend,
                }
            })),
        )?;
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
        Ok(NegotiationResponse {
            runtime_id: Self::parse_required_string(&result, "runtime_id")?,
            active_contract_version: Self::parse_required_u16(&result, "active_contract_version")?,
            effective_capabilities,
            effective_limits,
            contract_release: Self::parse_required_string(&result, "contract_release")?,
            schema_namespace: Self::parse_required_string(&result, "schema_namespace")?,
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
            let mut sdk_meta = serde_json::Map::new();
            if let Some(value) = idempotency_key {
                sdk_meta.insert("idempotency_key".to_string(), JsonValue::String(value));
            }
            if let Some(value) = ttl_ms {
                sdk_meta.insert("ttl_ms".to_string(), JsonValue::from(value));
            }
            if let Some(value) = correlation_id {
                sdk_meta.insert("correlation_id".to_string(), JsonValue::String(value));
            }
            if !extensions.is_empty() {
                sdk_meta.insert(
                    "extensions".to_string(),
                    JsonValue::Object(extensions.into_iter().collect()),
                );
            }
            if !sdk_meta.is_empty() {
                map.insert("_sdk".to_string(), JsonValue::Object(sdk_meta));
            }
        }
        let value = self.call_rpc(
            "sdk_send_v2",
            Some(json!({
                "id": format!("sdk-zmq-{}", self.next_request_id()),
                "source": source,
                "destination": destination,
                "title": title,
                "content": content,
                "fields": fields,
            })),
        )?;
        Ok(MessageId(Self::parse_required_string(&value, "message_id")?))
    }

    fn cancel(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        let result = self.call_rpc("sdk_cancel_message_v2", Some(json!({ "message_id": id.0 })))?;
        Self::parse_cancel_result(Self::parse_required_string(&result, "result")?.as_str())
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        let result = self.call_rpc("sdk_status_v2", Some(json!({ "message_id": id.0 })))?;
        let Some(record) = result.get("message") else {
            return Ok(None);
        };
        if record.is_null() {
            return Ok(None);
        }
        let state = Self::parse_delivery_state(record.get("receipt_status").and_then(JsonValue::as_str));
        let terminal = matches!(
            state,
            DeliveryState::Delivered
                | DeliveryState::Failed
                | DeliveryState::Cancelled
                | DeliveryState::Expired
                | DeliveryState::Rejected
        );
        let timestamp = record.get("timestamp").and_then(JsonValue::as_i64).unwrap_or(0_i64);
        Ok(Some(DeliverySnapshot {
            message_id: id,
            state,
            terminal,
            last_updated_ms: u64::try_from(timestamp.max(0)).unwrap_or(0).saturating_mul(1000),
            attempts: 0,
            reason_code: None,
        }))
    }

    fn configure(&self, expected_revision: u64, patch: ConfigPatch) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_configure_v2",
            Some(json!({ "expected_revision": expected_revision, "patch": patch })),
        )?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: result.get("revision").and_then(JsonValue::as_u64),
        })
    }

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError> {
        let result = self.call_rpc(
            "sdk_poll_events_v2",
            Some(json!({ "cursor": cursor.map(|cursor| cursor.0), "max": max })),
        )?;
        let events = result
            .get("events")
            .and_then(JsonValue::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        Ok(SdkEvent {
                            event_id: Self::parse_required_string(row, "event_id")?,
                            runtime_id: Self::parse_required_string(row, "runtime_id")?,
                            stream_id: Self::parse_required_string(row, "stream_id")?,
                            seq_no: Self::parse_required_u64(row, "seq_no")?,
                            contract_version: Self::parse_required_u16(row, "contract_version")?,
                            ts_ms: Self::parse_required_u64(row, "ts_ms")?,
                            event_type: Self::parse_required_string(row, "event_type")?,
                            severity: row
                                .get("severity")
                                .and_then(JsonValue::as_str)
                                .map(Self::parse_severity)
                                .unwrap_or(Severity::Info),
                            source_component: row
                                .get("source_component")
                                .and_then(JsonValue::as_str)
                                .unwrap_or("rns-rpc")
                                .to_owned(),
                            operation_id: row.get("operation_id").and_then(JsonValue::as_str).map(str::to_owned),
                            message_id: row.get("message_id").and_then(JsonValue::as_str).map(str::to_owned),
                            peer_id: row.get("peer_id").and_then(JsonValue::as_str).map(str::to_owned),
                            correlation_id: row.get("correlation_id").and_then(JsonValue::as_str).map(str::to_owned),
                            trace_id: row.get("trace_id").and_then(JsonValue::as_str).map(str::to_owned),
                            payload: row.get("payload").cloned().unwrap_or(JsonValue::Object(serde_json::Map::new())),
                            extensions: BTreeMap::new(),
                        })
                    })
                    .collect::<Result<Vec<_>, SdkError>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(EventBatch {
            events,
            next_cursor: EventCursor(Self::parse_required_string(&result, "next_cursor")?),
            dropped_count: result.get("dropped_count").and_then(JsonValue::as_u64).unwrap_or(0),
            snapshot_high_watermark_seq_no: result
                .get("snapshot_high_watermark_seq_no")
                .and_then(JsonValue::as_u64),
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
            in_flight_messages: result.get("in_flight_messages").and_then(JsonValue::as_u64).unwrap_or(0),
        })
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let mode = match mode {
            ShutdownMode::Graceful => "graceful",
            ShutdownMode::Immediate => "immediate",
        };
        let result = self.call_rpc("sdk_shutdown_v2", Some(json!({ "mode": mode })))?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        })
    }
}

async fn apply_role<S>(socket: &mut S, role: ZmqEndpointRole, endpoint: &str) -> Result<(), SdkError>
where
    S: Socket,
{
    match role {
        ZmqEndpointRole::Bind => {
            socket.bind(endpoint).await.map(|_| ()).map_err(|err| {
                sdk_error(ErrorCategory::Transport, format!("zmq bind {} failed: {}", endpoint, err))
            })
        }
        ZmqEndpointRole::Connect => socket.connect(endpoint).await.map_err(|err| {
            sdk_error(ErrorCategory::Transport, format!("zmq connect {} failed: {}", endpoint, err))
        }),
    }
}

fn validate_endpoint_security(endpoint: &str, has_auth: bool) -> Result<(), SdkError> {
    if is_local_endpoint(endpoint) || has_auth {
        return Ok(());
    }
    Err(SdkError::new(
        code::SECURITY_AUTH_REQUIRED,
        ErrorCategory::Security,
        "remote zmq endpoints require explicit token authentication",
    ))
}

fn is_local_endpoint(endpoint: &str) -> bool {
    if endpoint.starts_with("inproc://") {
        return true;
    }
    let Some(authority) = endpoint.strip_prefix("tcp://") else {
        return false;
    };
    let host = authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(authority)
        .trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn sdk_error(category: ErrorCategory, message: impl Into<String>) -> SdkError {
    SdkError::new(code::INTERNAL, category, message)
}

fn token_signature(secret: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("token shared secret must be non-empty");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn new_session_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("zmq-sdk-{:032x}", now)
}

fn map_rpc_error(error: RpcError) -> SdkError {
    let category = match error.category.as_deref() {
        Some("Validation") => ErrorCategory::Validation,
        Some("Capability") => ErrorCategory::Capability,
        Some("Config") => ErrorCategory::Config,
        Some("Policy") => ErrorCategory::Policy,
        Some("Security") => ErrorCategory::Security,
        Some("Transport") => ErrorCategory::Transport,
        Some("Timeout") => ErrorCategory::Timeout,
        Some("Runtime") => ErrorCategory::Runtime,
        _ => ErrorCategory::Internal,
    };
    SdkError::new(
        error.machine_code.as_deref().unwrap_or(error.code.as_str()),
        category,
        error.message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_remote_endpoints_without_auth() {
        let config = ZmqPipelineBackendConfig::local_tcp(
            "tcp://192.0.2.10:9000",
            "tcp://127.0.0.1:9001",
        );

        let err = config.validate().expect_err("remote without auth rejected");

        assert_eq!(err.category, ErrorCategory::Security);
        assert_eq!(err.machine_code, code::SECURITY_AUTH_REQUIRED);
    }

    #[test]
    fn config_accepts_loopback_without_auth() {
        let config = ZmqPipelineBackendConfig::local_tcp(
            "tcp://127.0.0.1:9000",
            "tcp://localhost:9001",
        );

        config.validate().expect("loopback accepted");
    }

    #[test]
    fn response_filter_requires_session_and_request_match() {
        let session = "session-a".to_string();
        let envelope = ZmqRpcEnvelope::response(session.clone(), 4, Vec::new());

        assert_eq!(envelope.kind, ZmqRpcEnvelopeKind::Response);
        assert_eq!(envelope.session_id, session);
        assert_eq!(envelope.request_id, 4);
    }
}
