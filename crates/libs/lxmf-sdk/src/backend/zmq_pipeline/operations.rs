use super::ZmqPipelineBackendClient;
use crate::app::{Envelope, EnvelopeResponse, OperationRegistry};
use crate::domain::{PaperDecodeResult, PaperMessageEnvelope};
use crate::error::{code, ErrorCategory, SdkError};
use crate::types::{Ack, MessageId, RuntimeSnapshot, RuntimeState, ShutdownMode};
use serde_json::{json, Value as JsonValue};

impl ZmqPipelineBackendClient {
    pub fn operation_registry(&self) -> Result<OperationRegistry, SdkError> {
        let result = self.call_rpc("sdk_operation_registry_v2", Some(json!({})))?;
        Self::decode_field_or_root(&result, "registry", "operation_registry response")
    }

    pub fn envelope_execute(&self, envelope: Envelope) -> Result<EnvelopeResponse, SdkError> {
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_envelope_execute_v2", Some(params))?;
        Self::decode_field_or_root(&result, "response", "envelope_execute response")
    }

    pub fn paper_encode(&self, message_id: MessageId) -> Result<PaperMessageEnvelope, SdkError> {
        let result =
            self.call_rpc("sdk_paper_encode_v2", Some(json!({ "message_id": message_id.0 })))?;
        Self::decode_field_or_root(&result, "envelope", "paper_encode response")
    }

    pub fn paper_decode(&self, envelope: PaperMessageEnvelope) -> Result<Ack, SdkError> {
        let result = self.paper_decode_rpc_result(envelope)?;
        Ok(Self::parse_ack(&result))
    }

    pub fn paper_decode_with_metadata(
        &self,
        envelope: PaperMessageEnvelope,
    ) -> Result<PaperDecodeResult, SdkError> {
        let result = self.paper_decode_rpc_result(envelope)?;
        Self::decode_field_or_root(&result, "paper", "paper_decode response")
    }

    fn paper_decode_rpc_result(
        &self,
        envelope: PaperMessageEnvelope,
    ) -> Result<JsonValue, SdkError> {
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        self.call_rpc("sdk_paper_decode_v2", Some(params))
    }

    pub fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
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

    pub fn shutdown(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
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
