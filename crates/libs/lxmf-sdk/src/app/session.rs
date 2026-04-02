use super::errors::Error;
#[cfg(feature = "sdk-async")]
use super::events::{map_event_batch, EventBatch};
use super::runtime::{Config, Profile, RunState};
#[cfg(feature = "sdk-async")]
use crate::SdkBackendAsyncEvents;
use crate::{
    Client as CoreClient, ClientHandle, EventCursor, LxmfSdk, RuntimeSnapshot, SdkBackend,
    ShutdownMode,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct SharedBackend<B>(pub(crate) Arc<B>);

impl<B: SdkBackend> SdkBackend for SharedBackend<B> {
    fn negotiate(
        &self,
        req: crate::NegotiationRequest,
    ) -> Result<crate::NegotiationResponse, crate::SdkError> {
        self.0.negotiate(req)
    }

    fn send(&self, req: crate::SendRequest) -> Result<crate::MessageId, crate::SdkError> {
        self.0.send(req)
    }

    fn cancel(&self, id: crate::MessageId) -> Result<crate::CancelResult, crate::SdkError> {
        self.0.cancel(id)
    }

    fn status(
        &self,
        id: crate::MessageId,
    ) -> Result<Option<crate::DeliverySnapshot>, crate::SdkError> {
        self.0.status(id)
    }

    fn configure(
        &self,
        expected_revision: u64,
        patch: crate::ConfigPatch,
    ) -> Result<crate::Ack, crate::SdkError> {
        self.0.configure(expected_revision, patch)
    }

    fn poll_events(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<crate::EventBatch, crate::SdkError> {
        self.0.poll_events(cursor, max)
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, crate::SdkError> {
        self.0.snapshot()
    }

    fn shutdown(&self, mode: ShutdownMode) -> Result<crate::Ack, crate::SdkError> {
        self.0.shutdown(mode)
    }

    fn tick(&self, budget: crate::TickBudget) -> Result<crate::TickResult, crate::SdkError> {
        self.0.tick(budget)
    }
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> SdkBackendAsyncEvents for SharedBackend<B> {
    fn subscribe_events(
        &self,
        start: crate::SubscriptionStart,
    ) -> Result<crate::EventSubscription, crate::SdkError> {
        self.0.subscribe_events(start)
    }
}

pub(crate) struct SessionState {
    pub(crate) handle: ClientHandle,
    pub(crate) config: Config,
    pub(crate) degraded: bool,
}

pub(crate) struct State<B: SdkBackend> {
    pub(crate) client: Option<Arc<CoreClient<SharedBackend<B>>>>,
    pub(crate) session: Option<Arc<Mutex<SessionState>>>,
    pub(crate) lifecycle: RunState,
}

impl<B: SdkBackend> Default for State<B> {
    fn default() -> Self {
        Self { client: None, session: None, lifecycle: RunState::New }
    }
}

#[cfg(feature = "sdk-async")]
pub struct EventStream<B: SdkBackendAsyncEvents> {
    pub(crate) client: Arc<CoreClient<SharedBackend<B>>>,
    pub(crate) session: Arc<Mutex<SessionState>>,
    pub(crate) cursor: Option<EventCursor>,
    pub(crate) max_batch_size: usize,
    pub(crate) profile: Profile,
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> EventStream<B> {
    pub fn next_batch(&mut self) -> Result<EventBatch, Error> {
        let batch = self
            .client
            .poll_events(self.cursor.clone(), self.max_batch_size)
            .map_err(Error::from)?;
        self.cursor = Some(batch.next_cursor.clone());

        let batch = map_event_batch(batch, self.profile.as_str());
        if batch.dropped_count > 0
            || batch
                .events
                .iter()
                .any(|event| matches!(event.kind, super::events::EventKind::StreamGapDetected(_)))
        {
            let mut session = self.session.lock().expect("app session mutex poisoned");
            session.degraded = true;
        }
        Ok(batch)
    }

    pub fn reset(&mut self) {
        self.cursor = None;
        let mut session = self.session.lock().expect("app session mutex poisoned");
        session.degraded = false;
    }
}
