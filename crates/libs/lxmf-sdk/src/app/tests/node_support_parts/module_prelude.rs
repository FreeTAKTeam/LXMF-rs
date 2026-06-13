pub(super) use crate::app::{
    AttemptDisposition, BootstrapRequest, Client, ContactUpdate, DeliveryOptions, Envelope,
    QueuePressureStrategy,
};

pub(super) use crate::app::{
    Config, DeliveryState, EnvelopeKind, OperationEntry, OperationKind, Profile, RunState,
    SendRequest, SubscriptionStart, TransportVariant,
};

pub(super) use crate::domain::TrustLevel;

pub(super) use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};

pub(super) use crate::event::{
    EventBatch as RawEventBatch, EventCursor, EventSubscription, SdkEvent, Severity as RawSeverity,
};

pub(super) use crate::{
    Ack, CancelResult, DeliverySnapshot, DeliveryState as RawDeliveryState, EffectiveLimits,
    NegotiationRequest, NegotiationResponse, Profile as CoreProfile, RuntimeSnapshot, RuntimeState,
    SdkBackend, SdkBackendAsyncEvents, SdkBackendAsyncOps, SendRequest as RawSendRequest,
    ShutdownMode,
};

pub(super) use serde_json::json;

pub(super) use std::collections::{BTreeMap, VecDeque};

use std::sync::atomic::{AtomicUsize, Ordering};

use std::sync::Mutex;

pub(super) struct MockBackend {
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
}

impl MockBackend {
    pub(super) fn new() -> Self {
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
        }
    }

    pub(super) fn new_paginated() -> Self {
        Self { paginate_discovery: true, ..Self::new() }
    }

    pub(super) fn queue_batch(&self, batch: RawEventBatch) {
        self.poll_batches.lock().expect("poll batches").push_back(batch);
    }

    pub(super) fn queue_live_event(&self, event: SdkEvent) {
        self.live_events.lock().expect("live events").push_back(Ok(event));
    }

    pub(super) fn queue_shutdown_result(&self, result: Result<Ack, SdkError>) {
        self.shutdown_results.lock().expect("shutdown results").push_back(result);
    }

    pub(super) fn queue_send_result(&self, result: Result<crate::MessageId, SdkError>) {
        self.send_results.lock().expect("send results").push_back(result);
    }

    pub(super) fn queue_remote_command_result(
        &self,
        result: Result<crate::domain::RemoteCommandResponse, SdkError>,
    ) {
        self.remote_command_results.lock().expect("remote command results").push_back(result);
    }

    pub(super) fn queue_envelope_result(
        &self,
        result: Result<crate::app::EnvelopeResponse, SdkError>,
    ) {
        self.envelope_results.lock().expect("envelope results").push_back(result);
    }

    pub(super) fn queue_voice_open_result(
        &self,
        result: Result<crate::domain::VoiceSessionId, SdkError>,
    ) {
        self.voice_open_results.lock().expect("voice open results").push_back(result);
    }

    pub(super) fn queue_voice_update_result(
        &self,
        result: Result<crate::domain::VoiceSessionState, SdkError>,
    ) {
        self.voice_update_results.lock().expect("voice update results").push_back(result);
    }

    pub(super) fn queue_voice_close_result(&self, result: Result<Ack, SdkError>) {
        self.voice_close_results.lock().expect("voice close results").push_back(result);
    }
}
