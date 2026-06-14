use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::domain::{IdentityAnnounceRequest, IdentityAnnounceResult};

impl ZmqPipelineBackendClient {
    pub fn identity_announce(
        &self,
        req: IdentityAnnounceRequest,
    ) -> Result<IdentityAnnounceResult, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_identity_announce_now_v2", Some(params))?;
        Self::decode_field_or_root(&result, "announce", "identity_announce response")
    }
}
