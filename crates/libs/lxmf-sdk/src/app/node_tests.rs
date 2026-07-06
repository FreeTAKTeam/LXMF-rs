use super::{Client, Envelope};
use crate::app::DeliveryOptions;
use crate::app::{
    Config, DeliveryState, EnvelopeKind, EventKind, OperationEntry, OperationKind, Profile,
    RunState, SendRequest, SubscriptionStart, TransportVariant,
};
use crate::domain::TrustLevel;
use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};
use crate::event::{
    EventBatch as RawEventBatch, EventCursor, EventSubscription, SdkEvent, Severity as RawSeverity,
};
use crate::{
    Ack, CancelResult, DeliverySnapshot, DeliveryState as RawDeliveryState, EffectiveLimits,
    NegotiationRequest, NegotiationResponse, Profile as CoreProfile, RuntimeSnapshot, RuntimeState,
    SdkBackend, SdkBackendAsyncEvents, SdkBackendAsyncOps, SendRequest as RawSendRequest,
    ShutdownMode,
};
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

struct MockBackend {
    runtime_seq: AtomicUsize,
    send_seq: AtomicUsize,
    paginate_discovery: bool,
    poll_batches: Mutex<VecDeque<RawEventBatch>>,
    live_events: Mutex<VecDeque<Result<SdkEvent, SdkError>>>,
    send_results: Mutex<VecDeque<Result<crate::MessageId, SdkError>>>,
    shutdown_calls: AtomicUsize,
    shutdown_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
    remote_command_results: Mutex<VecDeque<Result<crate::domain::RemoteCommandResponse, SdkError>>>,
    envelope_results: Mutex<VecDeque<Result<crate::app::EnvelopeResponse, SdkError>>>,
    voice_open_results: Mutex<VecDeque<Result<crate::domain::VoiceSessionId, SdkError>>>,
    voice_update_results: Mutex<VecDeque<Result<crate::domain::VoiceSessionState, SdkError>>>,
    voice_close_results: Mutex<VecDeque<Result<Ack, SdkError>>>,
    async_negotiate_gate: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

impl MockBackend {
    fn new() -> Self {
        Self {
            runtime_seq: AtomicUsize::new(1),
            send_seq: AtomicUsize::new(1),
            paginate_discovery: false,
            poll_batches: Mutex::new(VecDeque::new()),
            live_events: Mutex::new(VecDeque::new()),
            send_results: Mutex::new(VecDeque::new()),
            shutdown_calls: AtomicUsize::new(0),
            shutdown_results: Mutex::new(VecDeque::new()),
            remote_command_results: Mutex::new(VecDeque::new()),
            envelope_results: Mutex::new(VecDeque::new()),
            voice_open_results: Mutex::new(VecDeque::new()),
            voice_update_results: Mutex::new(VecDeque::new()),
            voice_close_results: Mutex::new(VecDeque::new()),
            async_negotiate_gate: Mutex::new(None),
        }
    }

    fn new_paginated() -> Self {
        Self { paginate_discovery: true, ..Self::new() }
    }

    fn queue_batch(&self, batch: RawEventBatch) {
        self.poll_batches.lock().expect("poll batches").push_back(batch);
    }

    fn queue_live_event(&self, event: SdkEvent) {
        self.live_events.lock().expect("live events").push_back(Ok(event));
    }

    fn queue_shutdown_result(&self, result: Result<Ack, SdkError>) {
        self.shutdown_results.lock().expect("shutdown results").push_back(result);
    }

    fn queue_send_result(&self, result: Result<crate::MessageId, SdkError>) {
        self.send_results.lock().expect("send results").push_back(result);
    }

    fn queue_remote_command_result(
        &self,
        result: Result<crate::domain::RemoteCommandResponse, SdkError>,
    ) {
        self.remote_command_results.lock().expect("remote command results").push_back(result);
    }

    fn queue_envelope_result(&self, result: Result<crate::app::EnvelopeResponse, SdkError>) {
        self.envelope_results.lock().expect("envelope results").push_back(result);
    }

    fn queue_voice_open_result(&self, result: Result<crate::domain::VoiceSessionId, SdkError>) {
        self.voice_open_results.lock().expect("voice open results").push_back(result);
    }

    fn queue_voice_update_result(
        &self,
        result: Result<crate::domain::VoiceSessionState, SdkError>,
    ) {
        self.voice_update_results.lock().expect("voice update results").push_back(result);
    }

    fn queue_voice_close_result(&self, result: Result<Ack, SdkError>) {
        self.voice_close_results.lock().expect("voice close results").push_back(result);
    }

    fn delay_next_async_negotiate(&self) -> tokio::sync::oneshot::Sender<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.async_negotiate_gate.lock().expect("async negotiate gate") = Some(rx);
        tx
    }
}

include!("node_tests_backend_core.rs");
include!("node_tests_backend_discovery_attachment.rs");
include!("node_tests_backend_topic_marker.rs");
include!("node_tests_backend_command_voice.rs");

impl SdkBackend for MockBackend {
    mock_backend_core_methods!();
    mock_backend_discovery_attachment_methods!();
    mock_backend_topic_marker_methods!();
    mock_backend_command_voice_methods!();
}

impl SdkBackendAsyncEvents for MockBackend {
    fn subscribe_events(
        &self,
        _start: crate::SubscriptionStart,
    ) -> Result<EventSubscription, SdkError> {
        Ok(EventSubscription {
            start: crate::SubscriptionStart::Head,
            cursor: Some(EventCursor("cursor-1".to_owned())),
        })
    }

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<crate::SdkEventStream>, SdkError> {
        let events = self.live_events.lock().expect("live events").drain(..).collect::<Vec<_>>();
        if events.is_empty() {
            return Ok(None);
        }
        Ok(Some(Box::pin(tokio_stream::iter(events))))
    }
}

impl SdkBackendAsyncOps for MockBackend {
    fn negotiate_async(
        &self,
        req: NegotiationRequest,
    ) -> crate::SdkBoxFuture<'_, NegotiationResponse> {
        let gate = self.async_negotiate_gate.lock().expect("async negotiate gate").take();
        Box::pin(async move {
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            self.negotiate(req)
        })
    }

    fn send_async(&self, req: RawSendRequest) -> crate::SdkBoxFuture<'_, crate::MessageId> {
        Box::pin(async move { self.send(req) })
    }

    fn status_async(
        &self,
        id: crate::MessageId,
    ) -> crate::SdkBoxFuture<'_, Option<DeliverySnapshot>> {
        Box::pin(async move { self.status(id) })
    }

    fn snapshot_async(&self) -> crate::SdkBoxFuture<'_, RuntimeSnapshot> {
        Box::pin(async move { self.snapshot() })
    }

    fn shutdown_async(&self, mode: ShutdownMode) -> crate::SdkBoxFuture<'_, Ack> {
        Box::pin(async move { self.shutdown(mode) })
    }
}

fn runtime_started_event() -> SdkEvent {
    SdkEvent {
        event_id: "evt-1".to_owned(),
        runtime_id: "rt-live".to_owned(),
        stream_id: "stream".to_owned(),
        seq_no: 1,
        contract_version: 2,
        ts_ms: 10,
        event_type: "RuntimeStateChanged".to_owned(),
        severity: RawSeverity::Info,
        source_component: "test".to_owned(),
        operation_id: None,
        message_id: None,
        peer_id: None,
        correlation_id: None,
        trace_id: None,
        payload: json!({ "from": "starting", "to": "running" }),
        extensions: BTreeMap::new(),
    }
}

fn stream_gap_event() -> SdkEvent {
    SdkEvent {
        event_id: "evt-2".to_owned(),
        runtime_id: "rt-live".to_owned(),
        stream_id: "stream".to_owned(),
        seq_no: 2,
        contract_version: 2,
        ts_ms: 20,
        event_type: "StreamGap".to_owned(),
        severity: RawSeverity::Warn,
        source_component: "test".to_owned(),
        operation_id: None,
        message_id: None,
        peer_id: None,
        correlation_id: None,
        trace_id: None,
        payload: json!({ "expected_seq_no": 3, "observed_seq_no": 6, "dropped_count": 3 }),
        extensions: BTreeMap::new(),
    }
}

include!("node_tests_envelope_routes.rs");
include!("node_tests_runtime_voice.rs");
include!("node_tests_client_domain.rs");
include!("node_tests_client_messages.rs");
