use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::app::{Envelope, EnvelopeResponse};
use crate::domain::{
    DeliveryStampPolicyRequest, DeliveryStampPolicyResult, PropagationAcknowledgeSyncRequest,
    PropagationAcknowledgeSyncResult, PropagationDeliveryPolicyEntryRequest,
    PropagationDeliveryPolicyRequest, PropagationDeliveryPolicyResult, PropagationEnableRequest,
    PropagationFetchRequest, PropagationFetchResult, PropagationIngestRequest,
    PropagationIngestResult, PropagationNodeListResult, PropagationNodeSelectionResult,
    PropagationNodeSetRequest, PropagationPeerMaintenanceResult, PropagationPeerSyncRequest,
    PropagationPeerSyncResult, PropagationRecoveryStateResult, PropagationRemotePeerRequest,
    PropagationRemoteRequest, PropagationRemoteStatusResult, PropagationRemoteSyncResult,
    PropagationRemoteTransferResult, PropagationRemoteUnpeerResult, PropagationStatusResult,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

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

    pub fn propagation_acknowledge_sync_completion(
        &self,
        req: PropagationAcknowledgeSyncRequest,
    ) -> Result<PropagationAcknowledgeSyncResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.acknowledge_sync_completion", req)
    }

    pub fn propagation_node_get(&self) -> Result<PropagationNodeSelectionResult, SdkError> {
        self.execute_propagation_query("app.propagation.node.get", json!({}))
    }

    pub fn propagation_node_set(
        &self,
        req: PropagationNodeSetRequest,
    ) -> Result<PropagationNodeSelectionResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.node.set", req)
    }

    pub fn propagation_node_list(&self) -> Result<PropagationNodeListResult, SdkError> {
        self.execute_propagation_query("app.propagation.node.list", json!({}))
    }

    pub fn propagation_status(&self) -> Result<PropagationStatusResult, SdkError> {
        self.execute_propagation_query("app.propagation.status", json!({}))
    }

    pub fn propagation_recovery_state(&self) -> Result<PropagationRecoveryStateResult, SdkError> {
        let status = self.propagation_status()?;
        PropagationRecoveryStateResult::try_from_propagation(status.propagation).map_err(|error| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                format!("invalid propagation recovery state: {error}"),
            )
        })
    }

    pub fn propagation_enable(
        &self,
        req: PropagationEnableRequest,
    ) -> Result<PropagationStatusResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.enable", req)
    }

    pub fn propagation_delivery_policy_get(
        &self,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        self.execute_propagation_query("app.propagation.delivery_policy.get", json!({}))
    }

    pub fn propagation_delivery_policy_set(
        &self,
        req: PropagationDeliveryPolicyRequest,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.delivery_policy.set", req)
    }

    pub fn propagation_delivery_policy_allow(
        &self,
        destination: impl Into<String>,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        let current = self.propagation_delivery_policy_get()?;
        self.propagation_delivery_policy_set(
            current.policy_state.request_with_allowed_destination(destination),
        )
    }

    pub fn propagation_delivery_policy_disallow(
        &self,
        destination: &str,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        let current = self.propagation_delivery_policy_get()?;
        self.propagation_delivery_policy_set(
            current.policy_state.request_without_allowed_destination(destination),
        )
    }

    pub fn propagation_delivery_policy_ignore(
        &self,
        destination: impl Into<String>,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        let current = self.propagation_delivery_policy_get()?;
        self.propagation_delivery_policy_set(
            current.policy_state.request_with_ignored_destination(destination),
        )
    }

    pub fn propagation_delivery_policy_unignore(
        &self,
        destination: &str,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        let current = self.propagation_delivery_policy_get()?;
        self.propagation_delivery_policy_set(
            current.policy_state.request_without_ignored_destination(destination),
        )
    }

    pub fn propagation_delivery_policy_prioritise(
        &self,
        destination: impl Into<String>,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        let current = self.propagation_delivery_policy_get()?;
        self.propagation_delivery_policy_set(
            current.policy_state.request_with_prioritised_destination(destination),
        )
    }

    pub fn propagation_delivery_policy_unprioritise(
        &self,
        destination: &str,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        let current = self.propagation_delivery_policy_get()?;
        self.propagation_delivery_policy_set(
            current.policy_state.request_without_prioritised_destination(destination),
        )
    }

    pub fn propagation_delivery_policy_allow_destination(
        &self,
        req: PropagationDeliveryPolicyEntryRequest,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.delivery_policy.allow_destination", req)
    }

    pub fn propagation_delivery_policy_disallow_destination(
        &self,
        req: PropagationDeliveryPolicyEntryRequest,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        self.execute_propagation_envelope(
            "app.propagation.delivery_policy.disallow_destination",
            req,
        )
    }

    pub fn propagation_delivery_policy_prioritise_destination(
        &self,
        req: PropagationDeliveryPolicyEntryRequest,
    ) -> Result<PropagationDeliveryPolicyResult, SdkError> {
        self.execute_propagation_envelope(
            "app.propagation.delivery_policy.prioritise_destination",
            req,
        )
    }

    pub fn delivery_stamp_policy_get(&self) -> Result<DeliveryStampPolicyResult, SdkError> {
        self.execute_propagation_query("app.delivery.stamp_policy.get", json!({}))
    }

    pub fn delivery_stamp_policy_set(
        &self,
        req: DeliveryStampPolicyRequest,
    ) -> Result<DeliveryStampPolicyResult, SdkError> {
        self.execute_propagation_envelope("app.delivery.stamp_policy.set", req)
    }

    pub fn propagation_peer_maintenance(
        &self,
    ) -> Result<PropagationPeerMaintenanceResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.peer_maintenance", json!({}))
    }

    pub fn propagation_ingest(
        &self,
        req: PropagationIngestRequest,
    ) -> Result<PropagationIngestResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.ingest", req)
    }

    pub fn propagation_fetch(
        &self,
        req: PropagationFetchRequest,
    ) -> Result<PropagationFetchResult, SdkError> {
        self.execute_propagation_envelope("app.propagation.fetch", req)
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
