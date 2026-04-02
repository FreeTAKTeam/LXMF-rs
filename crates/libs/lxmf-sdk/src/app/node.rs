use super::envelope::{Envelope, EnvelopeResponse};
use super::session::State;
use crate::SdkBackend;
use serde_json::Value as JsonValue;
use std::sync::{Arc, Mutex};

pub struct Client<B: SdkBackend> {
    pub(crate) backend: Arc<B>,
    pub(crate) state: Mutex<State<B>>,
}

impl<B: SdkBackend> Client<B> {
    pub fn new(backend: B) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    pub fn from_arc(backend: Arc<B>) -> Self {
        Self { backend, state: Mutex::new(State::default()) }
    }

    pub fn query(
        &self,
        operation_id: impl Into<super::operations::OperationId>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, super::errors::Error> {
        self.execute_envelope(Envelope::query(operation_id, payload))
    }

    pub fn command(
        &self,
        operation_id: impl Into<super::operations::OperationId>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, super::errors::Error> {
        self.execute_envelope(Envelope::command(operation_id, payload))
    }
}

#[cfg(feature = "rpc-backend")]
impl Client<crate::RpcBackendClient> {
    pub fn rpc(endpoint: impl Into<String>) -> Self {
        Self::new(crate::RpcBackendClient::new(endpoint.into()))
    }
}

#[cfg(test)]
#[path = "node_tests.rs"]
mod node_tests;
