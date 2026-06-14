use crate::app::{Envelope, EnvelopeResponse, OperationRegistry};
use crate::backend::SdkBackend;
use crate::capability::{NegotiationRequest, NegotiationResponse};
use crate::domain::{
    ContactListRequest, ContactListResult, ContactRecord, ContactUpdateRequest,
    IdentityAnnounceRequest, IdentityAnnounceResult, IdentityBootstrapRequest, IdentityBundle,
    IdentityImportRequest, IdentityRef, IdentityResolveRequest, PeerConnectionRequest,
    PeerConnectionResult, PresenceListRequest, PresenceListResult,
};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventBatch, EventCursor, SdkEvent, Severity};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState, MessageId, RuntimeSnapshot,
    RuntimeState, SendRequest, ShutdownMode,
};
use hmac::{Hmac, Mac};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_rpc_frame};
use rns_rpc::rpc::zmq::{self, ZmqRpcAuthMetadata, ZmqRpcEnvelope};
use rns_rpc::RpcError;
use serde_json::{json, Value as JsonValue};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

#[path = "zmq_pipeline/batch.rs"]
mod batch;
#[path = "zmq_pipeline/config.rs"]
mod config;
#[path = "zmq_pipeline/destination.rs"]
mod destination;
#[path = "zmq_pipeline/discovery.rs"]
mod discovery;
#[path = "zmq_pipeline/history.rs"]
mod history;
#[path = "zmq_pipeline/identity.rs"]
mod identity;
#[path = "zmq_pipeline/negotiation.rs"]
mod negotiation;
#[path = "zmq_pipeline/parsing.rs"]
mod parsing;
#[path = "zmq_pipeline/peer.rs"]
mod peer;
#[path = "zmq_pipeline/propagation.rs"]
mod propagation;
#[path = "zmq_pipeline/send.rs"]
mod send;
#[path = "zmq_pipeline/transport.rs"]
mod transport;
#[path = "zmq_pipeline/workflow.rs"]
mod workflow;

#[cfg(test)]
#[path = "zmq_pipeline/tests/mod.rs"]
mod tests;

pub use config::{ZmqEndpointRole, ZmqPipelineBackendConfig, ZmqPipelineTokenAuth};
use negotiation::new_session_id;
use transport::ZmqPipelineTransport;
pub struct ZmqPipelineBackendClient {
    config: ZmqPipelineBackendConfig,
    session_id: String,
    next_request_id: AtomicU64,
    negotiated_capabilities: RwLock<Vec<String>>,
    runtime: Runtime,
    transport: tokio::sync::Mutex<Option<ZmqPipelineTransport>>,
}
impl ZmqPipelineBackendClient {
    pub fn new(config: ZmqPipelineBackendConfig) -> Result<Self, SdkError> {
        config.validate()?;
        let runtime =
            Runtime::new().map_err(|err| sdk_error(ErrorCategory::Internal, err.to_string()))?;
        Ok(Self {
            config,
            session_id: new_session_id(),
            next_request_id: AtomicU64::new(1),
            negotiated_capabilities: RwLock::new(Vec::new()),
            runtime,
            transport: tokio::sync::Mutex::new(None),
        })
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }
    fn has_capability(&self, capability_id: &str) -> bool {
        self.negotiated_capabilities
            .read()
            .expect("negotiated_capabilities rwlock poisoned")
            .iter()
            .any(|capability| capability == capability_id)
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
            self.auth_metadata_for_request(request_id),
        );
        let encoded = zmq::encode_envelope(&envelope)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if encoded.len() > self.config.max_envelope_bytes {
            return Err(sdk_error(
                ErrorCategory::Transport,
                "zmq rpc envelope exceeded configured limit",
            ));
        }

        let response = self.runtime.block_on(self.send_and_recv(encoded, request_id))?;
        let rpc_response = parse_rpc_frame(&response.payload)
            .map_err(|err| sdk_error(ErrorCategory::Transport, err.to_string()))?;
        if let Some(error) = rpc_response.error {
            return Err(map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    fn auth_metadata_for_request(&self, request_id: u64) -> Option<ZmqRpcAuthMetadata> {
        let auth = self.config.token_auth.as_ref()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let payload = format!(
            "iss={};aud={};jti={}-{};sub=sdk-client;iat={};exp={}",
            auth.issuer,
            auth.audience,
            self.session_id,
            request_id,
            now,
            now.saturating_add(auth.ttl_secs.max(1)),
        );
        let sig = token_signature(auth.shared_secret.as_str(), payload.as_str());
        Some(ZmqRpcAuthMetadata {
            scheme: "bearer".to_string(),
            value: format!("{};sig={}", payload, sig),
        })
    }
}

impl SdkBackend for ZmqPipelineBackendClient {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let (bind_mode, auth_mode, rpc_backend) = self.negotiation_security_config(&req);
        let result = self.call_rpc(
            "sdk_negotiate_v2",
            Some(json!({
                "supported_contract_versions": req.supported_contract_versions,
                "requested_capabilities": req.requested_capabilities,
                "config": {
                    "profile": req.profile,
                    "bind_mode": bind_mode,
                    "auth_mode": auth_mode,
                    "overflow_policy": req.overflow_policy,
                    "block_timeout_ms": req.block_timeout_ms,
                    "rpc_backend": rpc_backend,
                    "extensions": req.extensions,
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
        *self.negotiated_capabilities.write().expect("negotiated_capabilities rwlock poisoned") =
            effective_capabilities.clone();
        Ok(NegotiationResponse {
            runtime_id: Self::parse_required_string(&result, "runtime_id")?,
            active_contract_version: Self::parse_required_u16(&result, "active_contract_version")?,
            effective_capabilities,
            effective_limits,
            contract_release: Self::parse_required_string(&result, "contract_release")?,
            schema_namespace: Self::parse_required_string(&result, "schema_namespace")?,
            sdk_version: Self::parse_optional_string_or_default(
                &result,
                "sdk_version",
                crate::SDK_VERSION,
            )?,
            python_reference: Self::parse_parity_reference(&result)?,
        })
    }

    fn send(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        let value = self.call_rpc(
            "sdk_send_v2",
            Some(send::send_params(req, format!("sdk-zmq-{}", self.next_request_id()))),
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
        let state =
            Self::parse_delivery_state(record.get("receipt_status").and_then(JsonValue::as_str));
        let terminal = match state {
            DeliveryState::Sent => !self.has_capability("sdk.capability.receipt_terminality"),
            DeliveryState::Delivered
            | DeliveryState::Failed
            | DeliveryState::Cancelled
            | DeliveryState::Expired
            | DeliveryState::Rejected => true,
            DeliveryState::Queued
            | DeliveryState::Dispatching
            | DeliveryState::InFlight
            | DeliveryState::Unknown => false,
        };
        let timestamp = record.get("timestamp").and_then(JsonValue::as_i64).unwrap_or(0_i64);
        Ok(Some(DeliverySnapshot {
            message_id: id,
            state,
            terminal,
            last_updated_ms: u64::try_from(timestamp.max(0)).unwrap_or(0).saturating_mul(1000),
            attempts: Self::parse_optional_u32(record, "attempts")?.unwrap_or(0),
            reason_code: Self::parse_optional_string(record, "reason_code")?,
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
                            operation_id: row
                                .get("operation_id")
                                .and_then(JsonValue::as_str)
                                .map(str::to_owned),
                            message_id: row
                                .get("message_id")
                                .and_then(JsonValue::as_str)
                                .map(str::to_owned),
                            peer_id: row
                                .get("peer_id")
                                .and_then(JsonValue::as_str)
                                .map(str::to_owned),
                            correlation_id: row
                                .get("correlation_id")
                                .and_then(JsonValue::as_str)
                                .map(str::to_owned),
                            trace_id: row
                                .get("trace_id")
                                .and_then(JsonValue::as_str)
                                .map(str::to_owned),
                            payload: row
                                .get("payload")
                                .cloned()
                                .unwrap_or(JsonValue::Object(serde_json::Map::new())),
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

    fn identity_announce_now(&self) -> Result<Ack, SdkError> {
        let result = self.call_rpc("sdk_identity_announce_now_v2", Some(json!({})))?;
        Ok(Self::parse_ack(&result))
    }

    fn identity_announce(
        &self,
        req: IdentityAnnounceRequest,
    ) -> Result<IdentityAnnounceResult, SdkError> {
        ZmqPipelineBackendClient::identity_announce(self, req)
    }

    fn identity_presence_list(
        &self,
        req: PresenceListRequest,
    ) -> Result<PresenceListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_presence_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "presence_list", "identity_presence_list response")
    }
    fn identity_list(&self) -> Result<Vec<IdentityBundle>, SdkError> {
        let result = self.call_rpc("sdk_identity_list_v2", Some(json!({})))?;
        Self::decode_field_or_root(&result, "identities", "identity_list response")
    }
    fn identity_activate(&self, identity: IdentityRef) -> Result<Ack, SdkError> {
        let result =
            self.call_rpc("sdk_identity_activate_v2", Some(json!({ "identity": identity.0 })))?;
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
        let result =
            self.call_rpc("sdk_identity_export_v2", Some(json!({ "identity": identity.0 })))?;
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
        if result.is_null() || result.get("identity").is_some_and(JsonValue::is_null) {
            return Ok(None);
        }
        let value = result.get("identity").cloned().unwrap_or(result);
        Self::decode_value(value, "identity_resolve response").map(Some)
    }
    fn identity_contact_update(
        &self,
        req: ContactUpdateRequest,
    ) -> Result<ContactRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_contact_update_v2", Some(params))?;
        Self::decode_field_or_root(&result, "contact", "identity_contact_update response")
    }
    fn identity_contact_list(
        &self,
        req: ContactListRequest,
    ) -> Result<ContactListResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_contact_list_v2", Some(params))?;
        Self::decode_field_or_root(&result, "contact_list", "identity_contact_list response")
    }
    fn identity_bootstrap(&self, req: IdentityBootstrapRequest) -> Result<ContactRecord, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_bootstrap_v2", Some(params))?;
        Self::decode_field_or_root(&result, "contact", "identity_bootstrap response")
    }
    fn peer_connect(&self, req: PeerConnectionRequest) -> Result<PeerConnectionResult, SdkError> {
        ZmqPipelineBackendClient::peer_connect(self, req)
    }
    fn peer_disconnect(
        &self,
        req: PeerConnectionRequest,
    ) -> Result<PeerConnectionResult, SdkError> {
        ZmqPipelineBackendClient::peer_disconnect(self, req)
    }
    fn peer_reconnect(&self, req: PeerConnectionRequest) -> Result<PeerConnectionResult, SdkError> {
        ZmqPipelineBackendClient::peer_reconnect(self, req)
    }
    fn operation_registry(&self) -> Result<OperationRegistry, SdkError> {
        let result = self.call_rpc("sdk_operation_registry_v2", Some(json!({})))?;
        Self::decode_field_or_root(&result, "registry", "operation_registry response")
    }
    fn envelope_execute(&self, envelope: Envelope) -> Result<EnvelopeResponse, SdkError> {
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_envelope_execute_v2", Some(params))?;
        Self::decode_field_or_root(&result, "response", "envelope_execute response")
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
        let result = self.call_rpc("sdk_shutdown_v2", Some(json!({ "mode": mode })))?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        })
    }
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
