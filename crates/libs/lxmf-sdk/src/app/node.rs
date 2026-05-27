use super::envelope::{Envelope, EnvelopeResponse};
use super::session::State;
#[cfg(feature = "sdk-async")]
use super::surfaces::Events;
use super::surfaces::{Attachments, IdentityDirectory, Messages, Runtime};
use crate::SdkBackend;
use serde_json::Value as JsonValue;
use std::sync::{Arc, Mutex};

pub struct Client<B: SdkBackend> {
    pub(crate) backend: Arc<B>,
    pub(crate) state: Arc<Mutex<State<B>>>,
}

impl<B: SdkBackend> Clone for Client<B> {
    fn clone(&self) -> Self {
        Self { backend: Arc::clone(&self.backend), state: Arc::clone(&self.state) }
    }
}

impl<B: SdkBackend> Client<B> {
    pub fn new(backend: B) -> Self {
        Self::from_arc(Arc::new(backend))
    }

    pub fn from_arc(backend: Arc<B>) -> Self {
        Self { backend, state: Arc::new(Mutex::new(State::default())) }
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

    pub fn messages(&self) -> Messages<'_, B> {
        Messages::new(self)
    }

    #[cfg(feature = "sdk-async")]
    pub fn events(&self) -> Events<'_, B>
    where
        B: crate::SdkBackendAsyncEvents,
    {
        Events::new(self)
    }

    pub fn identity(&self) -> IdentityDirectory<'_, B> {
        IdentityDirectory::new(self)
    }

    pub fn attachments(&self) -> Attachments<'_, B> {
        Attachments::new(self)
    }

    pub fn runtime(&self) -> Runtime<'_, B> {
        Runtime::new(self)
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
