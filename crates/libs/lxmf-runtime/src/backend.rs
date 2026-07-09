use std::sync::{Arc, Mutex};

use lxmf_sdk::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState, EventBatch, EventCursor,
    MessageId, NegotiationRequest, NegotiationResponse, RuntimeSnapshot, SdkBackend, SdkError,
    SendRequest, Severity, ShutdownMode,
};
use rns_transport::hash::AddressHash;
use serde_json::{json, Value as JsonValue};

use crate::config::InProcessBackendConfig;
use crate::delivery::{
    request_link_attempts, request_link_timeout, request_resource_timeout, InProcessSendReport,
    SendContext,
};
use crate::state::{internal_error, BackendState};

#[derive(Clone)]
pub struct InProcessBackend {
    config: Arc<InProcessBackendConfig>,
    state: Arc<Mutex<BackendState>>,
    propagation_relay: Arc<Mutex<Option<AddressHash>>>,
}

impl InProcessBackend {
    pub fn new(config: InProcessBackendConfig) -> Self {
        let state = BackendState::new(config.runtime_id.clone(), config.limits);
        let propagation_relay = Arc::new(Mutex::new(config.propagation_relay));
        Self { config: Arc::new(config), state: Arc::new(Mutex::new(state)), propagation_relay }
    }

    pub fn send_report(
        &self,
        message_id: &MessageId,
    ) -> Result<Option<InProcessSendReport>, SdkError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .send_report(&message_id.0))
    }

    pub fn set_propagation_relay(&self, relay: Option<AddressHash>) -> Result<(), SdkError> {
        *self
            .propagation_relay
            .lock()
            .map_err(|_| internal_error("in-process propagation relay state poisoned"))? = relay;
        Ok(())
    }

    pub fn record_delivery(
        &self,
        message_id: &MessageId,
        state: DeliveryState,
        reason: Option<String>,
    ) -> Result<(), SdkError> {
        self.state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .record_delivery(&message_id.0, state, reason)
    }

    pub fn record_event(
        &self,
        event_type: &str,
        severity: Severity,
        payload: JsonValue,
    ) -> Result<(), SdkError> {
        self.state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .record_event(event_type, severity, payload)
    }
}

impl SdkBackend for InProcessBackend {
    fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let runtime_id = self
            .state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .runtime_id()
            .to_owned();
        serde_json::from_value(json!({
            "runtime_id": runtime_id,
            "active_contract_version": 2,
            "effective_capabilities": [
                "sdk.capability.event_stream",
                "sdk.capability.cursor_replay",
                "sdk.capability.receipt_terminality",
                "sdk.capability.config_revision_cas",
                "reticulum.capability.raw_bytes",
                "reticulum.capability.msgpack_fields"
            ],
            "effective_limits": {
                "max_poll_events": 128,
                "max_event_bytes": 65536,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 16,
                "idempotency_ttl_ms": 43200000
            },
            "contract_release": lxmf_sdk::CONTRACT_RELEASE,
            "schema_namespace": lxmf_sdk::SCHEMA_NAMESPACE,
        }))
        .map_err(|err| internal_error(format!("invalid negotiation response: {err}")))
    }

    fn send(&self, request: SendRequest) -> Result<MessageId, SdkError> {
        let link_timeout = request_link_timeout(&request, self.config.link_connect_timeout);
        let link_attempts = request_link_attempts(&request, self.config.link_connect_attempts);
        let resource_timeout =
            request_resource_timeout(&request, self.config.resource_transfer_timeout);
        let context = SendContext {
            transport: &self.config.transport,
            identity: &self.config.identity,
            source_destination: self.config.source_destination,
            propagation_relay: *self
                .propagation_relay
                .lock()
                .map_err(|_| internal_error("in-process propagation relay state poisoned"))?,
            link_connect_timeout: link_timeout,
            link_connect_attempts: link_attempts,
            resource_transfer_timeout: resource_timeout,
        };
        let result = self.config.runtime_handle.block_on(crate::delivery::send(context, &request));
        match result {
            Ok(report) => {
                let message_id = report.message_id.clone();
                self.state
                    .lock()
                    .map_err(|_| internal_error("in-process backend state poisoned"))?
                    .record_send(report)?;
                Ok(message_id)
            }
            Err(error) => Err(error),
        }
    }

    fn cancel(&self, _id: MessageId) -> Result<CancelResult, SdkError> {
        Ok(CancelResult::Unsupported)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .status(&id))
    }

    fn configure(&self, _expected_revision: u64, _patch: ConfigPatch) -> Result<Ack, SdkError> {
        let revision = self
            .state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .advance_config_revision();
        make_ack(Some(revision))
    }

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError> {
        self.state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .poll(cursor.as_ref(), max)
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        self.state
            .lock()
            .map_err(|_| internal_error("in-process backend state poisoned"))?
            .snapshot()
    }

    fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
        make_ack(None)
    }
}

fn make_ack(revision: Option<u64>) -> Result<Ack, SdkError> {
    serde_json::from_value(json!({
        "accepted": true,
        "revision": revision,
    }))
    .map_err(|err| internal_error(format!("invalid acknowledgement: {err}")))
}
