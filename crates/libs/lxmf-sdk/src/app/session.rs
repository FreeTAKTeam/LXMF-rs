use super::errors::Error;
#[cfg(feature = "sdk-async")]
use super::errors::{ErrorCategory, ErrorCode};
#[cfg(feature = "sdk-async")]
use super::events::{map_event_batch, Event, EventBatch};
use super::runtime::{Config, Profile, RunState};
use crate::{
    Client as CoreClient, ClientHandle, EventCursor, LxmfSdk, RuntimeSnapshot, SdkBackend,
    ShutdownMode,
};
#[cfg(feature = "sdk-async")]
use crate::{SdkBackendAsyncEvents, SdkBackendAsyncOps};
#[cfg(feature = "sdk-async")]
use std::collections::{BTreeMap, VecDeque};
#[cfg(feature = "sdk-async")]
use std::future::Future;
#[cfg(feature = "sdk-async")]
use std::pin::Pin;
use std::sync::{Arc, Mutex};
#[cfg(feature = "sdk-async")]
use std::task::{Context, Poll};
#[cfg(feature = "sdk-async")]
use std::time::Duration;
#[cfg(feature = "sdk-async")]
use tokio::task::JoinHandle;
#[cfg(feature = "sdk-async")]
use tokio_stream::Stream;

#[cfg(feature = "sdk-async")]
type EventBatchFetchResult = Result<(EventBatch, Option<EventCursor>), Error>;
#[cfg(feature = "sdk-async")]
type EventBatchFetchHandle = JoinHandle<EventBatchFetchResult>;

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
impl<B: SdkBackendAsyncOps> SdkBackendAsyncOps for SharedBackend<B> {
    fn negotiate_async(
        &self,
        req: crate::NegotiationRequest,
    ) -> crate::SdkBoxFuture<'_, crate::NegotiationResponse> {
        self.0.negotiate_async(req)
    }

    fn send_async(&self, req: crate::SendRequest) -> crate::SdkBoxFuture<'_, crate::MessageId> {
        self.0.send_async(req)
    }

    fn status_async(
        &self,
        id: crate::MessageId,
    ) -> crate::SdkBoxFuture<'_, Option<crate::DeliverySnapshot>> {
        self.0.status_async(id)
    }

    fn snapshot_async(&self) -> crate::SdkBoxFuture<'_, RuntimeSnapshot> {
        self.0.snapshot_async()
    }

    fn shutdown_async(&self, mode: ShutdownMode) -> crate::SdkBoxFuture<'_, crate::Ack> {
        self.0.shutdown_async(mode)
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

    fn open_event_stream(
        &self,
        subscription: &crate::EventSubscription,
    ) -> Result<Option<crate::SdkEventStream>, crate::SdkError> {
        self.0.open_event_stream(subscription)
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
    pub(crate) pending_events: VecDeque<Event>,
    pub(crate) inflight: Option<EventBatchFetchHandle>,
    pub(crate) live_stream: Option<crate::SdkEventStream>,
    pub(crate) last_seq_no: Option<u64>,
    pub(crate) idle_delay: Duration,
    pub(crate) idle_sleep: Option<Pin<Box<tokio::time::Sleep>>>,
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> EventStream<B> {
    fn fetch_next_batch(
        client: Arc<CoreClient<SharedBackend<B>>>,
        session: Arc<Mutex<SessionState>>,
        cursor: Option<EventCursor>,
        max_batch_size: usize,
        profile: Profile,
    ) -> EventBatchFetchResult {
        let batch = client.poll_events(cursor, max_batch_size).map_err(Error::from)?;
        let next_cursor = Some(batch.next_cursor.clone());

        let batch = map_event_batch(batch, profile.as_str());
        if batch.dropped_count > 0
            || batch
                .events
                .iter()
                .any(|event| matches!(event.kind, super::events::EventKind::StreamGapDetected(_)))
        {
            let mut session = session.lock().expect("app session mutex poisoned");
            session.degraded = true;
        }
        Ok((batch, next_cursor))
    }

    fn map_live_event(
        raw_event: crate::SdkEvent,
        session: Arc<Mutex<SessionState>>,
        profile: Profile,
    ) -> Result<(Event, EventCursor), Error> {
        let cursor = EventCursor(format!(
            "v2:{}:{}:{}",
            raw_event.runtime_id, raw_event.stream_id, raw_event.seq_no
        ));
        let raw_batch = crate::EventBatch {
            events: vec![raw_event],
            next_cursor: cursor.clone(),
            dropped_count: 0,
            snapshot_high_watermark_seq_no: None,
            extensions: BTreeMap::new(),
        };
        let mut batch = map_event_batch(raw_batch, profile.as_str());
        let event = batch.events.pop().ok_or_else(|| {
            Self::stream_internal_error("event stream returned an empty live event batch")
        })?;
        if matches!(event.kind, super::events::EventKind::StreamGapDetected(_)) {
            let mut session = session.lock().expect("app session mutex poisoned");
            session.degraded = true;
        }
        Ok((event, cursor))
    }

    fn stream_internal_error(message: impl Into<String>) -> Error {
        Error {
            code: ErrorCode::InternalUnexpectedFailure,
            category: ErrorCategory::Internal,
            retryable: false,
            terminal: true,
            user_action_required: false,
            message: message.into(),
            details: BTreeMap::new(),
            cause_code: None,
        }
    }

    pub fn next_batch(&mut self) -> Result<EventBatch, Error> {
        let (batch, next_cursor) = Self::fetch_next_batch(
            Arc::clone(&self.client),
            Arc::clone(&self.session),
            self.cursor.clone(),
            self.max_batch_size,
            self.profile.clone(),
        )?;
        self.cursor = next_cursor;
        self.last_seq_no = batch.events.iter().map(|event| event.metadata.seq_no).max();
        Ok(batch)
    }

    pub fn reset(&mut self) {
        self.cursor = None;
        self.last_seq_no = None;
        self.pending_events.clear();
        if let Some(inflight) = self.inflight.take() {
            inflight.abort();
        }
        self.idle_sleep = None;
        let mut session = self.session.lock().expect("app session mutex poisoned");
        session.degraded = false;
    }
}

#[cfg(feature = "sdk-async")]
impl<B: SdkBackendAsyncEvents> Drop for EventStream<B> {
    fn drop(&mut self) {
        if let Some(inflight) = self.inflight.take() {
            inflight.abort();
        }
    }
}

#[cfg(feature = "sdk-async")]
impl<B> Stream for EventStream<B>
where
    B: SdkBackendAsyncEvents + 'static,
{
    type Item = Result<Event, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(event) = this.pending_events.pop_front() {
                return Poll::Ready(Some(Ok(event)));
            }

            if let Some(stream) = this.live_stream.as_mut() {
                match stream.as_mut().poll_next(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Some(Ok(raw_event))) => {
                        if this
                            .last_seq_no
                            .is_some_and(|last_seq_no| raw_event.seq_no <= last_seq_no)
                        {
                            continue;
                        }
                        match Self::map_live_event(
                            raw_event,
                            Arc::clone(&this.session),
                            this.profile.clone(),
                        ) {
                            Ok((event, cursor)) => {
                                this.last_seq_no = Some(event.metadata.seq_no);
                                this.cursor = Some(cursor);
                                return Poll::Ready(Some(Ok(event)));
                            }
                            Err(err) => return Poll::Ready(Some(Err(err))),
                        }
                    }
                    Poll::Ready(Some(Err(err))) => {
                        this.live_stream = None;
                        return Poll::Ready(Some(Err(Error::from(err))));
                    }
                    Poll::Ready(None) => {
                        this.live_stream = None;
                    }
                }
            }

            if let Some(sleep) = this.idle_sleep.as_mut() {
                if sleep.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                this.idle_sleep = None;
            }

            if this.inflight.is_none() {
                let client = Arc::clone(&this.client);
                let session = Arc::clone(&this.session);
                let cursor = this.cursor.clone();
                let max_batch_size = this.max_batch_size;
                let profile = this.profile.clone();
                this.inflight = Some(tokio::task::spawn_blocking(move || {
                    Self::fetch_next_batch(client, session, cursor, max_batch_size, profile)
                }));
            }

            let Some(inflight) = this.inflight.as_mut() else {
                continue;
            };
            match Pin::new(inflight).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(joined) => {
                    this.inflight = None;
                    match joined {
                        Ok(Ok((batch, next_cursor))) => {
                            this.cursor = next_cursor;
                            this.pending_events = batch
                                .events
                                .into_iter()
                                .filter(|event| match this.last_seq_no {
                                    Some(last_seq_no) => event.metadata.seq_no > last_seq_no,
                                    None => true,
                                })
                                .collect();
                            if let Some(last_seq_no) =
                                this.pending_events.iter().map(|event| event.metadata.seq_no).max()
                            {
                                this.last_seq_no = Some(last_seq_no);
                            }
                            if this.pending_events.is_empty() {
                                this.idle_sleep =
                                    Some(Box::pin(tokio::time::sleep(this.idle_delay)));
                            }
                        }
                        Ok(Err(err)) => return Poll::Ready(Some(Err(err))),
                        Err(err) => {
                            return Poll::Ready(Some(Err(Self::stream_internal_error(format!(
                                "event stream worker failed: {err}"
                            )))));
                        }
                    }
                }
            }
        }
    }
}
