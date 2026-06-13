use super::{send, SdkError, ZmqPipelineBackendClient};
use crate::types::{BatchSendRequest, BatchSendResult};

impl ZmqPipelineBackendClient {
    pub fn send_batch(&self, req: BatchSendRequest) -> Result<BatchSendResult, SdkError> {
        let result = self.call_rpc("sdk_send_batch_v2", Some(send::batch_params(req)))?;
        Self::decode_value(result, "batch send response")
    }
}
