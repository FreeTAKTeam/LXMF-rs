use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::app::{Envelope, EnvelopeResponse};
use crate::domain::{
    PropagationPeerSyncRequest, PropagationPeerSyncResult, PropagationRemotePeerRequest,
    PropagationRemoteRequest, PropagationRemoteStatusResult, PropagationRemoteSyncResult,
    PropagationRemoteTransferResult, PropagationRemoteUnpeerResult,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

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

    pub fn propagation_remote_status(
        &self,
        req: PropagationRemoteRequest,
    ) -> Result<PropagationRemoteStatusResult, SdkError> {
        self.execute_propagation_query("app.propagation.remote_status", req)
    }

    pub fn propagation_remote_fetch(
        &self,
        req: PropagationRemoteRequest,
    ) -> Result<PropagationRemoteTransferResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.remote_fetch", req)
    }

    pub fn propagation_remote_download(
        &self,
        req: PropagationRemoteRequest,
    ) -> Result<PropagationRemoteTransferResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.remote_download", req)
    }

    pub fn propagation_remote_sync(
        &self,
        req: PropagationRemotePeerRequest,
    ) -> Result<PropagationRemoteSyncResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.remote_sync", req)
    }

    pub fn propagation_remote_unpeer(
        &self,
        req: PropagationRemotePeerRequest,
    ) -> Result<PropagationRemoteUnpeerResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.remote_unpeer", req)
    }

    fn execute_propagation_envelope<T, R>(
        &self,
        operation_id: &'static str,
        req: R,
    ) -> Result<T, SdkError>
    where
        T: DeserializeOwned,
        R: Serialize,
    {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let envelope = Envelope::command(operation_id, params);
        self.execute_propagation_envelope_request(envelope)
    }

    fn execute_propagation_query<T, R>(
        &self,
        operation_id: &'static str,
        req: R,
    ) -> Result<T, SdkError>
    where
        T: DeserializeOwned,
        R: Serialize,
    {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let envelope = Envelope::query(operation_id, params);
        self.execute_propagation_envelope_request(envelope)
    }

    fn execute_propagation_envelope_request<T>(&self, envelope: Envelope) -> Result<T, SdkError>
    where
        T: DeserializeOwned,
    {
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_envelope_execute_v2", Some(params))?;
        let response: EnvelopeResponse =
            Self::decode_field_or_root(&result, "response", "propagation response")?;
        Self::decode_value(response.payload, "propagation payload")
    }
}
