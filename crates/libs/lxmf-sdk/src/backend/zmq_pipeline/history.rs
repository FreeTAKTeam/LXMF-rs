use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::messaging::{MessageHistoryListRequest, MessageHistoryPage};

impl ZmqPipelineBackendClient {
    pub fn list_message_history(
        &self,
        req: MessageHistoryListRequest,
    ) -> Result<MessageHistoryPage, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("list_messages", Some(params))?;
        Self::decode_value(result, "message history response")
    }
}
