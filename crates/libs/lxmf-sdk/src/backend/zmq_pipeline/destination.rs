use super::{json, SdkError, ZmqPipelineBackendClient};

impl ZmqPipelineBackendClient {
    pub fn local_delivery_destination_hash(&self) -> Result<String, SdkError> {
        let result = self.call_rpc("status", Some(json!({})))?;
        Self::parse_required_string(&result, "delivery_destination_hash")
    }
}
