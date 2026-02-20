use crate::backend::SdkBackend;
#[cfg(feature = "sdk-async")]
use crate::backend::SdkBackendAsyncEvents;
use crate::capability::{EffectiveLimits, NegotiationRequest, NegotiationResponse};
use crate::domain::{
    AttachmentId, AttachmentListRequest, AttachmentListResult, AttachmentMeta,
    AttachmentStoreRequest, IdentityBundle, IdentityImportRequest, IdentityRef,
    IdentityResolveRequest, MarkerCreateRequest, MarkerId, MarkerListRequest, MarkerListResult,
    MarkerRecord, MarkerUpdatePositionRequest, PaperMessageEnvelope, RemoteCommandRequest,
    RemoteCommandResponse, TelemetryPoint, TelemetryQuery, TopicCreateRequest, TopicId,
    TopicListRequest, TopicListResult, TopicPublishRequest, TopicRecord, TopicSubscriptionRequest,
    VoiceSessionId, VoiceSessionOpenRequest, VoiceSessionState, VoiceSessionUpdateRequest,
};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor, SdkEvent, Severity};
#[cfg(feature = "sdk-async")]
use crate::event::{EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, AuthMode, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState, MessageId,
    RuntimeSnapshot, RuntimeState, SendRequest, ShutdownMode, TickBudget, TickResult,
};
use hmac::{Hmac, Mac};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_http_response_body, parse_rpc_frame};
use rns_rpc::RpcError;
use serde::de::DeserializeOwned;
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RpcBackendClient {
    endpoint: String,
    next_request_id: AtomicU64,
    negotiated_capabilities: RwLock<Vec<String>>,
    session_auth: RwLock<SessionAuth>,
}

#[derive(Clone, Debug)]
enum SessionAuth {
    LocalTrusted,
    Token { issuer: String, audience: String, shared_secret: String, ttl_secs: u64 },
    Mtls { allowed_san: Option<String> },
}

impl RpcBackendClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            next_request_id: AtomicU64::new(1),
            negotiated_capabilities: RwLock::new(Vec::new()),
            session_auth: RwLock::new(SessionAuth::LocalTrusted),
        }
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn now_seconds() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs()).unwrap_or(0)
    }

    fn call_rpc(&self, method: &str, params: Option<JsonValue>) -> Result<JsonValue, SdkError> {
        let auth = self.session_auth.read().expect("session_auth rwlock poisoned").clone();
        let headers = self.headers_for_session_auth(&auth);
        self.call_rpc_with_headers(method, params, &headers)
    }

    fn call_rpc_with_fallback(
        &self,
        primary_method: &str,
        fallback_method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        match self.call_rpc(primary_method, params.clone()) {
            Ok(result) => Ok(result),
            Err(err) if err.machine_code == "NOT_IMPLEMENTED" => {
                self.call_rpc(fallback_method, params)
            }
            Err(err) => Err(err),
        }
    }

    fn call_rpc_with_headers(
        &self,
        method: &str,
        params: Option<JsonValue>,
        headers: &[(String, String)],
    ) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let frame = build_rpc_frame(request_id, method, params).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let request = Self::build_http_post_with_headers("/rpc", &self.endpoint, &frame, headers);
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

    fn build_http_post_with_headers(
        path: &str,
        host: &str,
        body: &[u8],
        headers: &[(String, String)],
    ) -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(format!("POST {path} HTTP/1.1\r\n").as_bytes());
        request.extend_from_slice(format!("Host: {host}\r\n").as_bytes());
        request.extend_from_slice(b"Content-Type: application/msgpack\r\n");
        for (name, value) in headers {
            request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        request.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
        request.extend_from_slice(b"\r\n");
        request.extend_from_slice(body);
        request
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

    fn bind_mode_to_wire(bind_mode: crate::types::BindMode) -> &'static str {
        match bind_mode {
            crate::types::BindMode::LocalOnly => "local_only",
            crate::types::BindMode::Remote => "remote",
        }
    }

    fn auth_mode_to_wire(auth_mode: crate::types::AuthMode) -> &'static str {
        match auth_mode {
            crate::types::AuthMode::LocalTrusted => "local_trusted",
            crate::types::AuthMode::Token => "token",
            crate::types::AuthMode::Mtls => "mtls",
        }
    }

    fn overflow_policy_to_wire(overflow_policy: crate::types::OverflowPolicy) -> &'static str {
        match overflow_policy {
            crate::types::OverflowPolicy::Reject => "reject",
            crate::types::OverflowPolicy::DropOldest => "drop_oldest",
            crate::types::OverflowPolicy::Block => "block",
        }
    }

    fn session_auth_from_request(&self, req: &NegotiationRequest) -> Result<SessionAuth, SdkError> {
        match req.auth_mode {
            AuthMode::LocalTrusted => Ok(SessionAuth::LocalTrusted),
            AuthMode::Mtls => {
                let mtls_auth = req
                    .rpc_backend
                    .as_ref()
                    .and_then(|config| config.mtls_auth.as_ref())
                    .ok_or_else(|| {
                        SdkError::new(
                            code::SECURITY_AUTH_REQUIRED,
                            ErrorCategory::Security,
                            "mtls auth mode requires rpc_backend.mtls_auth",
                        )
                    })?;
                if mtls_auth.ca_bundle_path.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode requires non-empty rpc_backend.mtls_auth.ca_bundle_path",
                    ));
                }
                Ok(SessionAuth::Mtls { allowed_san: mtls_auth.allowed_san.clone() })
            }
            AuthMode::Token => {
                let token_auth = req
                    .rpc_backend
                    .as_ref()
                    .and_then(|config| config.token_auth.as_ref())
                    .ok_or_else(|| {
                        SdkError::new(
                            code::SECURITY_AUTH_REQUIRED,
                            ErrorCategory::Security,
                            "token auth mode requires rpc_backend.token_auth",
                        )
                    })?;
                if token_auth.shared_secret.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "token auth shared_secret must be configured",
                    ));
                }
                Ok(SessionAuth::Token {
                    issuer: token_auth.issuer.clone(),
                    audience: token_auth.audience.clone(),
                    shared_secret: token_auth.shared_secret.clone(),
                    ttl_secs: (token_auth.jti_cache_ttl_ms / 1000).max(1),
                })
            }
        }
    }

    fn token_signature(secret: &str, payload: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("token shared secret must be non-empty");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn headers_for_session_auth(&self, auth: &SessionAuth) -> Vec<(String, String)> {
        match auth {
            SessionAuth::LocalTrusted => Vec::new(),
            SessionAuth::Mtls { allowed_san } => {
                let mut headers = vec![("X-Client-Cert-Present".to_owned(), "1".to_owned())];
                if let Some(allowed_san) =
                    allowed_san.as_deref().map(str::trim).filter(|value| !value.is_empty())
                {
                    headers.push(("X-Client-SAN".to_owned(), allowed_san.to_owned()));
                }
                headers.push(("X-Client-Subject".to_owned(), "sdk-client-mtls".to_owned()));
                headers
            }
            SessionAuth::Token { issuer, audience, shared_secret, ttl_secs } => {
                let jti = format!("sdk-jti-{}", self.next_request_id());
                let iat = Self::now_seconds();
                let exp = iat.saturating_add(*ttl_secs);
                let payload = format!(
                    "iss={issuer};aud={audience};jti={jti};sub=sdk-client;iat={iat};exp={exp}"
                );
                let sig = Self::token_signature(shared_secret, payload.as_str());
                let token = format!("{payload};sig={sig}");
                vec![("Authorization".to_owned(), format!("Bearer {token}"))]
            }
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

    fn decode_value<T: DeserializeOwned>(value: JsonValue, context: &str) -> Result<T, SdkError> {
        serde_json::from_value(value).map_err(|err| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("failed to decode {context}: {err}"),
            )
        })
    }

    fn decode_field_or_root<T: DeserializeOwned>(
        result: &JsonValue,
        field: &str,
        context: &str,
    ) -> Result<T, SdkError> {
        let value = result.get(field).cloned().unwrap_or_else(|| result.clone());
        Self::decode_value(value, context)
    }

    fn decode_optional_field<T: DeserializeOwned>(
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

    fn parse_ack(result: &JsonValue) -> Ack {
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
        let session_auth = self.session_auth_from_request(&req)?;
        let headers = self.headers_for_session_auth(&session_auth);
        let rpc_backend = req.rpc_backend.as_ref().map(|config| {
            json!({
                "listen_addr": config.listen_addr,
                "read_timeout_ms": config.read_timeout_ms,
                "write_timeout_ms": config.write_timeout_ms,
                "max_header_bytes": config.max_header_bytes,
                "max_body_bytes": config.max_body_bytes,
                "token_auth": config.token_auth.as_ref().map(|token| json!({
                    "issuer": token.issuer,
                    "audience": token.audience,
                    "jti_cache_ttl_ms": token.jti_cache_ttl_ms,
                    "clock_skew_ms": token.clock_skew_ms,
                    "shared_secret": token.shared_secret,
                })),
                "mtls_auth": config.mtls_auth.as_ref().map(|mtls| json!({
                    "ca_bundle_path": mtls.ca_bundle_path,
                    "require_client_cert": mtls.require_client_cert,
                    "allowed_san": mtls.allowed_san,
                })),
            })
        });
        let result = self.call_rpc_with_headers(
            "sdk_negotiate_v2",
            Some(json!({
                "supported_contract_versions": req.supported_contract_versions,
                "requested_capabilities": req.requested_capabilities,
                "config": {
                    "profile": Self::profile_to_wire(req.profile),
                    "bind_mode": Self::bind_mode_to_wire(req.bind_mode),
                    "auth_mode": Self::auth_mode_to_wire(req.auth_mode),
                    "overflow_policy": Self::overflow_policy_to_wire(req.overflow_policy),
                    "block_timeout_ms": req.block_timeout_ms,
                    "rpc_backend": rpc_backend,
                }
            })),
            &headers,
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
        {
            let mut guard = self.session_auth.write().expect("session_auth rwlock poisoned");
            *guard = session_auth;
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

        let result = self.call_rpc_with_fallback(
            "sdk_send_v2",
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

    fn topic_create(&self, req: TopicCreateRequest) -> Result<TopicRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_create_v2", Some(params))?;
        Self::decode_field_or_root(&result, "topic", "topic_create response")
    }

    fn topic_get(&self, topic_id: TopicId) -> Result<Option<TopicRecord>, SdkError> {
        let result = self.call_rpc(
            "sdk_topic_get_v2",
            Some(json!({
                "topic_id": topic_id.0,
            })),
        )?;
        if result.get("topic").is_some() {
            return Self::decode_optional_field(&result, "topic", "topic_get response");
        }
        if result.is_null() {
            return Ok(None);
        }
        Self::decode_value(result, "topic_get response").map(Some)
    }

    fn topic_list(&self, req: TopicListRequest) -> Result<TopicListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "topic_list", "topic_list response")
    }

    fn topic_subscribe(&self, req: TopicSubscriptionRequest) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_subscribe_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    fn topic_unsubscribe(&self, topic_id: TopicId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_topic_unsubscribe_v2",
            Some(json!({
                "topic_id": topic_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    fn topic_publish(&self, req: TopicPublishRequest) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_topic_publish_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    fn telemetry_query(&self, query: TelemetryQuery) -> Result<Vec<TelemetryPoint>, SdkError> {
        let params = serde_json::to_value(query).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_telemetry_query_v2", Some(params))?;
        if let Some(points) = result.get("points") {
            return Self::decode_value(points.clone(), "telemetry_query points");
        }
        Self::decode_value(result, "telemetry_query points")
    }

    fn telemetry_subscribe(&self, query: TelemetryQuery) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(query).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_telemetry_subscribe_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    fn attachment_store(&self, req: AttachmentStoreRequest) -> Result<AttachmentMeta, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_store_v2", Some(params))?;
        Self::decode_field_or_root(&result, "attachment", "attachment_store response")
    }

    fn attachment_get(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<Option<AttachmentMeta>, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_get_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
            })),
        )?;
        if result.get("attachment").is_some() {
            return Self::decode_optional_field(&result, "attachment", "attachment_get response");
        }
        if result.is_null() {
            return Ok(None);
        }
        Self::decode_value(result, "attachment_get response").map(Some)
    }

    fn attachment_list(
        &self,
        req: AttachmentListRequest,
    ) -> Result<AttachmentListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_attachment_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "attachment_list", "attachment_list response")
    }

    fn attachment_delete(&self, attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_delete_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    fn attachment_download(&self, attachment_id: AttachmentId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_download_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    fn attachment_associate_topic(
        &self,
        attachment_id: AttachmentId,
        topic_id: TopicId,
    ) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_attachment_associate_topic_v2",
            Some(json!({
                "attachment_id": attachment_id.0,
                "topic_id": topic_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    fn marker_create(&self, req: MarkerCreateRequest) -> Result<MarkerRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_marker_create_v2", Some(params))?;
        Self::decode_field_or_root(&result, "marker", "marker_create response")
    }

    fn marker_list(&self, req: MarkerListRequest) -> Result<MarkerListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_marker_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "marker_list", "marker_list response")
    }

    fn marker_update_position(
        &self,
        req: MarkerUpdatePositionRequest,
    ) -> Result<MarkerRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_marker_update_position_v2", Some(params))?;
        Self::decode_field_or_root(&result, "marker", "marker_update_position response")
    }

    fn marker_delete(&self, marker_id: MarkerId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_marker_delete_v2",
            Some(json!({
                "marker_id": marker_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    fn identity_list(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        let result = self.call_rpc("sdk_identity_list_v2", Some(json!({})))?;
        if let Some(identities) = result.get("identities") {
            return Self::decode_value(identities.clone(), "identity_list response");
        }
        Self::decode_value(result, "identity_list response")
    }

    fn identity_activate(&self, identity: IdentityRef) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_identity_activate_v2",
            Some(json!({
                "identity": identity.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
    }

    fn identity_import(&self, req: IdentityImportRequest) -> Result<IdentityBundle, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_import_v2", Some(params))?;
        Self::decode_field_or_root(&result, "identity", "identity_import response")
    }

    fn identity_export(&self, identity: IdentityRef) -> Result<IdentityImportRequest, SdkError> {
        let result = self.call_rpc(
            "sdk_identity_export_v2",
            Some(json!({
                "identity": identity.0,
            })),
        )?;
        Self::decode_field_or_root(&result, "bundle", "identity_export response")
    }

    fn identity_resolve(
        &self,
        req: IdentityResolveRequest,
    ) -> Result<Option<IdentityRef>, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_resolve_v2", Some(params))?;
        if result.get("identity").is_some() {
            return Self::decode_optional_field(&result, "identity", "identity_resolve response");
        }
        if result.is_null() {
            return Ok(None);
        }
        Self::decode_value(result, "identity_resolve response").map(Some)
    }

    fn paper_encode(&self, message_id: MessageId) -> Result<PaperMessageEnvelope, SdkError> {
        let result = self.call_rpc(
            "sdk_paper_encode_v2",
            Some(json!({
                "message_id": message_id.0,
            })),
        )?;
        Self::decode_field_or_root(&result, "envelope", "paper_encode response")
    }

    fn paper_decode(&self, envelope: PaperMessageEnvelope) -> Result<Ack, SdkError> {
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_paper_decode_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    fn command_invoke(&self, req: RemoteCommandRequest) -> Result<RemoteCommandResponse, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_command_invoke_v2", Some(params))?;
        Self::decode_field_or_root(&result, "response", "command_invoke response")
    }

    fn command_reply(
        &self,
        correlation_id: String,
        reply: RemoteCommandResponse,
    ) -> Result<Ack, SdkError> {
        let mut params = serde_json::to_value(reply).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        if let Some(object) = params.as_object_mut() {
            object.insert("correlation_id".to_owned(), JsonValue::String(correlation_id));
        } else {
            return Err(SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                "command_reply payload serialization did not produce an object",
            ));
        }
        let result = self.call_rpc("sdk_command_reply_v2", Some(params))?;
        Ok(Self::parse_ack(&result))
    }

    fn voice_session_open(&self, req: VoiceSessionOpenRequest) -> Result<VoiceSessionId, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_voice_session_open_v2", Some(params))?;
        if let Some(session_id) = result.get("session_id").and_then(JsonValue::as_str) {
            return Ok(VoiceSessionId(session_id.to_owned()));
        }
        Self::decode_value(result, "voice_session_open response")
    }

    fn voice_session_update(
        &self,
        req: VoiceSessionUpdateRequest,
    ) -> Result<VoiceSessionState, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_voice_session_update_v2", Some(params))?;
        if let Some(state) = result.get("state") {
            return Self::decode_value(state.clone(), "voice_session_update response");
        }
        Self::decode_value(result, "voice_session_update response")
    }

    fn voice_session_close(&self, session_id: VoiceSessionId) -> Result<Ack, SdkError> {
        let result = self.call_rpc(
            "sdk_voice_session_close_v2",
            Some(json!({
                "session_id": session_id.0,
            })),
        )?;
        Ok(Self::parse_ack(&result))
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
