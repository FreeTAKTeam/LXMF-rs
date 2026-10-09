use super::*;
use rns_rpc::broker::*;

impl ZmqPipelineBackendClient {
    /// This opt-in protocol never changes the negotiation/configuration of other SDKs.
    pub fn negotiate_durable_broker(&self) -> Result<NegotiationResponse, SdkError> {
        let reply =
            self.call_rpc("sdk_broker_negotiate_v1", Some(json!({"ack_meaning":"stored"})))?;
        if reply.get("capability").and_then(JsonValue::as_str) != Some(CAPABILITY)
            || reply.get("ack_meaning").and_then(JsonValue::as_str) != Some("stored")
        {
            return Err(SdkError::new(
                "SDK_BROKER_NEGOTIATION_REQUIRED",
                ErrorCategory::Capability,
                "daemon did not negotiate durable custody",
            ));
        }
        let negotiation: NegotiationResponse = serde_json::from_value(reply).map_err(|e| {
            SdkError::new("SDK_BROKER_RESPONSE_INVALID", ErrorCategory::Transport, e.to_string())
        })?;
        *self.negotiated_capabilities.write().expect("negotiated capabilities poisoned") =
            negotiation.effective_capabilities.clone();
        Ok(negotiation)
    }
    fn broker_replay_safe<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: JsonValue,
    ) -> Result<T, SdkError> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            sdk_error(ErrorCategory::Internal, "sync broker call attempted on async-only client")
        })?;
        let response = runtime.block_on(self.call_rpc_replay_safe(method, Some(params)))?;
        serde_json::from_value(response).map_err(|e| {
            SdkError::new("SDK_BROKER_RESPONSE_INVALID", ErrorCategory::Transport, e.to_string())
        })
    }
    pub fn broker_announces(
        &self,
        request: AnnounceProjectionRequest,
    ) -> Result<JsonValue, SdkError> {
        self.broker_replay_safe("sdk_broker_announces_v1", json!(request))
    }
    pub fn broker_admit(&self, request: AdmitRequest) -> Result<OperationReceipt, SdkError> {
        // A lost mutation reply is reconciled by operation ID, never a blind legacy send retry.
        let value = self.call_rpc("sdk_broker_admit_v1", Some(json!(request)))?;
        serde_json::from_value(value).map_err(|e| {
            SdkError::new("SDK_BROKER_RESPONSE_INVALID", ErrorCategory::Transport, e.to_string())
        })
    }
    pub fn broker_reconcile(
        &self,
        request: ReconcileRequest,
    ) -> Result<Option<OperationReceipt>, SdkError> {
        self.broker_replay_safe("sdk_broker_reconcile_v1", json!(request))
    }
    pub fn broker_resume(&self, request: ResumeRequest) -> Result<BrokerCheckpoint, SdkError> {
        self.broker_replay_safe("sdk_broker_resume_v1", json!(request))
    }
    pub fn broker_fetch(&self, request: FetchRequest) -> Result<BrokerBatch, SdkError> {
        self.broker_replay_safe("sdk_broker_fetch_v1", json!(request))
    }
    pub fn broker_ack_stored(&self, request: AckStoredRequest) -> Result<EventPosition, SdkError> {
        let reply: JsonValue =
            self.broker_replay_safe("sdk_broker_ack_stored_v1", json!(request))?;
        reply.get("stored").and_then(JsonValue::as_u64).map(EventPosition).ok_or_else(|| {
            SdkError::new(
                "SDK_BROKER_RESPONSE_INVALID",
                ErrorCategory::Transport,
                "missing stored checkpoint",
            )
        })
    }
}
