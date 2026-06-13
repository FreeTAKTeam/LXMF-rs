use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::domain::{PeerConnectionRequest, PeerConnectionResult};

impl ZmqPipelineBackendClient {
    pub fn peer_connect(
        &self,
        req: PeerConnectionRequest,
    ) -> Result<PeerConnectionResult, SdkError> {
        self.peer_lifecycle_call("sdk_peer_connect_v2", req, "peer_connect response")
    }

    pub fn peer_disconnect(
        &self,
        req: PeerConnectionRequest,
    ) -> Result<PeerConnectionResult, SdkError> {
        self.peer_lifecycle_call("sdk_peer_disconnect_v2", req, "peer_disconnect response")
    }

    pub fn peer_reconnect(
        &self,
        req: PeerConnectionRequest,
    ) -> Result<PeerConnectionResult, SdkError> {
        self.peer_lifecycle_call("sdk_peer_reconnect_v2", req, "peer_reconnect response")
    }

    fn peer_lifecycle_call(
        &self,
        method: &str,
        req: PeerConnectionRequest,
        context: &str,
    ) -> Result<PeerConnectionResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc(method, Some(params))?;
        Self::decode_field_or_root(&result, "peer", context)
    }
}
