use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::domain::{WorkflowPeerReadyRequest, WorkflowPeerReadyResult};

impl ZmqPipelineBackendClient {
    pub fn workflow_peer_ready(
        &self,
        req: WorkflowPeerReadyRequest,
    ) -> Result<WorkflowPeerReadyResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_workflow_peer_ready_v2", Some(params))?;
        Self::decode_field_or_root(&result, "workflow", "workflow_peer_ready response")
    }
}
