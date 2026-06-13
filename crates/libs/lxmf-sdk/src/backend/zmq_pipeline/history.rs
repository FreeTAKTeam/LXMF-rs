use super::{code, ErrorCategory, SdkError, ZmqPipelineBackendClient};
use crate::app::{Envelope, EnvelopeResponse};
use crate::messaging::{MessageHistoryListRequest, MessageHistoryPage};

impl ZmqPipelineBackendClient {
    pub fn list_message_history(
        &self,
        req: MessageHistoryListRequest,
    ) -> Result<MessageHistoryPage, SdkError> {
        let params = serde_json::to_value(req).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let envelope = Envelope::query("app.message.history.list", params);
        let params = serde_json::to_value(envelope).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc("sdk_envelope_execute_v2", Some(params))?;
        let response: EnvelopeResponse =
            Self::decode_field_or_root(&result, "response", "message history envelope response")?;
        Self::decode_value(response.payload, "message history response")
    }
}
