use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::app::{Envelope, EnvelopeResponse};
use crate::domain::{PropagationPeerSyncRequest, PropagationPeerSyncResult};

impl ZmqPipelineBackendClient {
    pub fn propagation_peer_sync(
        &self,
        req: PropagationPeerSyncRequest,
    ) -> Result<PropagationPeerSyncResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let envelope = Envelope::command("app.propagation.peer_sync", params);
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_envelope_execute_v2", Some(params))?;
        let response: EnvelopeResponse =
            Self::decode_field_or_root(&result, "response", "propagation peer sync response")?;
        Self::decode_value(response.payload, "propagation peer sync payload")
    }
}
