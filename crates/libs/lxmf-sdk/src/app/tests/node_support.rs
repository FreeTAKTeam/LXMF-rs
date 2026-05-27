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

impl SdkBackend for MockBackend {
    fn negotiate(&self, req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        let runtime_id = format!("rt-{}", self.runtime_seq.fetch_add(1, Ordering::Relaxed));
        let mut effective_capabilities = crate::required_capabilities(req.profile)
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect::<Vec<_>>();
        if !effective_capabilities
            .iter()
            .any(|capability| capability == "sdk.capability.async_events")
        {
            effective_capabilities.push("sdk.capability.async_events".to_owned());
        }
        for capability in [
            "sdk.capability.identity_multi",
            "sdk.capability.identity_discovery",
            "sdk.capability.contact_management",
        ] {
            if !effective_capabilities.iter().any(|current| current == capability) {
                effective_capabilities.push(capability.to_owned());
            }
        }
        Ok(NegotiationResponse {
            runtime_id,
            active_contract_version: 2,
            effective_capabilities,
            effective_limits: EffectiveLimits {
                max_poll_events: 32,
                max_event_bytes: 8_192,
                max_batch_bytes: 65_536,
                max_extension_keys: 32,
                idempotency_ttl_ms: 60_000,
            },
            contract_release: "v2.5".to_owned(),
            schema_namespace: "v2".to_owned(),
        })
    }

    fn send(&self, _req: RawSendRequest) -> Result<crate::MessageId, SdkError> {
        self.send_results.lock().expect("send results").pop_front().unwrap_or_else(|| {
            Ok(crate::MessageId(format!("msg-{}", self.send_seq.fetch_add(1, Ordering::Relaxed))))
        })
    }

    fn cancel(&self, _id: crate::MessageId) -> Result<CancelResult, SdkError> {
        Ok(CancelResult::Accepted)
    }

    fn status(&self, id: crate::MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        Ok(Some(DeliverySnapshot {
            message_id: id,
            state: RawDeliveryState::Sent,
            terminal: false,
            last_updated_ms: 10,
            attempts: 1,
            reason_code: None,
        }))
    }

    fn configure(
        &self,
        _expected_revision: u64,
        _patch: crate::ConfigPatch,
    ) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: Some(1) })
    }

    fn poll_events(
        &self,
        cursor: Option<EventCursor>,
        _max: usize,
    ) -> Result<RawEventBatch, SdkError> {
        self.poll_batches
            .lock()
            .expect("poll batches")
            .pop_front()
            .ok_or_else(|| {
                SdkError::new(code::RUNTIME_STREAM_DEGRADED, SdkErrorCategory::Runtime, "empty")
                    .with_retryable(false)
            })
            .or_else(|_| {
                Ok(RawEventBatch::empty(
                    cursor.unwrap_or_else(|| EventCursor("cursor-0".to_owned())),
                ))
            })
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        Ok(RuntimeSnapshot {
            runtime_id: "rt-live".to_owned(),
            state: RuntimeState::Running,
            active_contract_version: 2,
            event_stream_position: 7,
            config_revision: 1,
            queued_messages: 1,
            in_flight_messages: 2,
        })
    }

    fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
        self.shutdown_calls.fetch_add(1, Ordering::Relaxed);
        self.shutdown_results
            .lock()
            .expect("shutdown results")
            .pop_front()
            .unwrap_or(Ok(Ack { accepted: true, revision: None }))
    }

    fn identity_list(&self) -> Result<Vec<crate::domain::IdentityBundle>, SdkError> {
        Ok(vec![crate::domain::IdentityBundle {
            identity: crate::domain::IdentityRef("alice".to_owned()),
            public_key: "pubkey".to_owned(),
            display_name: Some("Alice".to_owned()),
            capabilities: vec!["chat".to_owned()],
            extensions: BTreeMap::new(),
        }])
    }

    fn identity_contact_list(
        &self,
        req: crate::domain::ContactListRequest,
    ) -> Result<crate::domain::ContactListResult, SdkError> {
        let make_contact = |identity: &str, display_name: &str, trust_level, bootstrap| {
            crate::domain::ContactRecord {
                identity: crate::domain::IdentityRef(identity.to_owned()),
                display_name: Some(display_name.to_owned()),
                trust_level,
                bootstrap,
                updated_ts_ms: 100,
                metadata: BTreeMap::new(),
                extensions: BTreeMap::from([("cursor".to_owned(), serde_json::json!(req.cursor))]),
            }
        };
        if self.paginate_discovery {
            return Ok(match req.cursor.as_deref() {
                None => crate::domain::ContactListResult {
                    contacts: vec![make_contact(
                        "bob",
                        "Bob",
                        crate::domain::TrustLevel::Trusted,
                        true,
                    )],
                    next_cursor: Some("contact:1".to_owned()),
                },
                Some("contact:1") => crate::domain::ContactListResult {
                    contacts: vec![make_contact(
                        "charlie",
                        "Charlie",
                        crate::domain::TrustLevel::Untrusted,
                        false,
                    )],
                    next_cursor: None,
                },
                _ => crate::domain::ContactListResult { contacts: Vec::new(), next_cursor: None },
            });
        }
        Ok(crate::domain::ContactListResult {
            contacts: vec![make_contact("bob", "Bob", crate::domain::TrustLevel::Trusted, true)],
            next_cursor: None,
        })
    }

    fn identity_announce_now(&self) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: None })
    }

    fn identity_presence_list(
        &self,
        _req: crate::domain::PresenceListRequest,
    ) -> Result<crate::domain::PresenceListResult, SdkError> {
        let req = _req;
        let bob = crate::domain::PresenceRecord {
            peer_id: "bob".to_owned(),
            last_seen_ts_ms: 200,
            first_seen_ts_ms: 120,
            seen_count: 3,
            name: Some("Bob Relay".to_owned()),
            name_source: Some("announce".to_owned()),
            trust_level: Some(crate::domain::TrustLevel::Trusted),
            bootstrap: Some(true),
            extensions: BTreeMap::from([("source".to_owned(), serde_json::json!("presence"))]),
        };
        let eve = crate::domain::PresenceRecord {
            peer_id: "eve".to_owned(),
            last_seen_ts_ms: 99,
            first_seen_ts_ms: 90,
            seen_count: 1,
            name: Some("Eve".to_owned()),
            name_source: Some("announce".to_owned()),
            trust_level: Some(crate::domain::TrustLevel::Unknown),
            bootstrap: Some(false),
            extensions: BTreeMap::new(),
        };
        if self.paginate_discovery {
            return Ok(match req.cursor.as_deref() {
                None => crate::domain::PresenceListResult {
                    peers: vec![bob],
                    next_cursor: Some("presence:1".to_owned()),
                },
                Some("presence:1") => {
                    crate::domain::PresenceListResult { peers: vec![eve], next_cursor: None }
                }
                _ => crate::domain::PresenceListResult { peers: Vec::new(), next_cursor: None },
            });
        }
        Ok(crate::domain::PresenceListResult { peers: vec![bob, eve], next_cursor: None })
    }

    fn identity_contact_update(
        &self,
        req: crate::domain::ContactUpdateRequest,
    ) -> Result<crate::domain::ContactRecord, SdkError> {
        Ok(crate::domain::ContactRecord {
            identity: req.identity,
            display_name: req.display_name,
            trust_level: req.trust_level.unwrap_or(crate::domain::TrustLevel::Unknown),
            bootstrap: req.bootstrap.unwrap_or(false),
            updated_ts_ms: 500,
            metadata: req.metadata,
            extensions: req.extensions,
        })
    }

    fn identity_bootstrap(
        &self,
        req: crate::domain::IdentityBootstrapRequest,
    ) -> Result<crate::domain::ContactRecord, SdkError> {
        Ok(crate::domain::ContactRecord {
            identity: req.identity,
            display_name: None,
            trust_level: crate::domain::TrustLevel::Trusted,
            bootstrap: true,
            updated_ts_ms: 600,
            metadata: BTreeMap::new(),
            extensions: req.extensions,
        })
    }

    fn attachment_store(
        &self,
        req: crate::domain::AttachmentStoreRequest,
    ) -> Result<crate::domain::AttachmentMeta, SdkError> {
        Ok(crate::domain::AttachmentMeta {
            attachment_id: crate::domain::AttachmentId("attachment-1".to_owned()),
            name: req.name,
            content_type: req.content_type,
            byte_len: 11,
            checksum_sha256: "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c"
                .to_owned(),
            created_ts_ms: 650,
            expires_ts_ms: req.expires_ts_ms,
            topic_ids: req.topic_ids,
            extensions: req.extensions,
        })
    }

    fn attachment_get(
        &self,
        attachment_id: crate::domain::AttachmentId,
    ) -> Result<Option<crate::domain::AttachmentMeta>, SdkError> {
        Ok(Some(crate::domain::AttachmentMeta {
            attachment_id,
            name: "sample.txt".to_owned(),
            content_type: "text/plain".to_owned(),
            byte_len: 11,
            checksum_sha256: "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c"
                .to_owned(),
            created_ts_ms: 651,
            expires_ts_ms: None,
            topic_ids: vec![crate::domain::TopicId("topic-1".to_owned())],
            extensions: BTreeMap::new(),
        }))
    }

    fn attachment_list(
        &self,
        req: crate::domain::AttachmentListRequest,
    ) -> Result<crate::domain::AttachmentListResult, SdkError> {
        Ok(crate::domain::AttachmentListResult {
            attachments: vec![crate::domain::AttachmentMeta {
                attachment_id: crate::domain::AttachmentId("attachment-1".to_owned()),
                name: "sample.txt".to_owned(),
                content_type: "text/plain".to_owned(),
                byte_len: 11,
                checksum_sha256: "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c"
                    .to_owned(),
                created_ts_ms: 652,
                expires_ts_ms: None,
                topic_ids: req.topic_id.into_iter().collect(),
                extensions: BTreeMap::new(),
            }],
            next_cursor: None,
        })
    }

    fn attachment_delete(
        &self,
        _attachment_id: crate::domain::AttachmentId,
    ) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: None })
    }

    fn attachment_upload_start(
        &self,
        _req: crate::domain::AttachmentUploadStartRequest,
    ) -> Result<crate::domain::AttachmentUploadSession, SdkError> {
        Ok(crate::domain::AttachmentUploadSession {
            upload_id: crate::domain::AttachmentUploadId("upload-1".to_owned()),
            attachment_id: crate::domain::AttachmentId("attachment-2".to_owned()),
            chunk_size_hint: 65_536,
            next_offset: 0,
        })
    }

    fn attachment_upload_chunk(
        &self,
        req: crate::domain::AttachmentUploadChunkRequest,
    ) -> Result<crate::domain::AttachmentUploadChunkAck, SdkError> {
        let complete = req.offset.saturating_add(5) >= 11;
        Ok(crate::domain::AttachmentUploadChunkAck {
            accepted: true,
            next_offset: req.offset.saturating_add(5),
            complete,
        })
    }

    fn attachment_upload_commit(
        &self,
        req: crate::domain::AttachmentUploadCommitRequest,
    ) -> Result<crate::domain::AttachmentMeta, SdkError> {
        Ok(crate::domain::AttachmentMeta {
            attachment_id: crate::domain::AttachmentId(
                req.upload_id.0.replace("upload", "attachment"),
            ),
            name: "chunked.bin".to_owned(),
            content_type: "application/octet-stream".to_owned(),
            byte_len: 11,
            checksum_sha256: "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c"
                .to_owned(),
            created_ts_ms: 653,
            expires_ts_ms: None,
            topic_ids: vec![crate::domain::TopicId("topic-1".to_owned())],
            extensions: req.extensions,
        })
    }

    fn attachment_download_chunk(
        &self,
        req: crate::domain::AttachmentDownloadChunkRequest,
    ) -> Result<crate::domain::AttachmentDownloadChunk, SdkError> {
        let done = req.offset > 0;
        Ok(crate::domain::AttachmentDownloadChunk {
            attachment_id: req.attachment_id,
            offset: req.offset,
            next_offset: if done { 11 } else { 5 },
            total_size: 11,
            done,
            checksum_sha256: "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c"
                .to_owned(),
            bytes_base64: if done { "IHdvcmxk".to_owned() } else { "aGVsbG8=".to_owned() },
        })
    }

    fn attachment_associate_topic(
        &self,
        _attachment_id: crate::domain::AttachmentId,
        _topic_id: crate::domain::TopicId,
    ) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: None })
    }

    fn topic_create(
        &self,
        req: crate::domain::TopicCreateRequest,
    ) -> Result<crate::domain::TopicRecord, SdkError> {
        Ok(crate::domain::TopicRecord {
            topic_id: crate::domain::TopicId("topic-1".to_owned()),
            topic_path: req.topic_path,
            created_ts_ms: 700,
            metadata: req.metadata,
            extensions: req.extensions,
        })
    }

    fn topic_get(
        &self,
        topic_id: crate::domain::TopicId,
    ) -> Result<Option<crate::domain::TopicRecord>, SdkError> {
        Ok(Some(crate::domain::TopicRecord {
            topic_id,
            topic_path: Some(crate::domain::TopicPath("ops/alerts".to_owned())),
            created_ts_ms: 700,
            metadata: BTreeMap::from([("kind".to_owned(), serde_json::json!("ops"))]),
            extensions: BTreeMap::new(),
        }))
    }

    fn topic_list(
        &self,
        req: crate::domain::TopicListRequest,
    ) -> Result<crate::domain::TopicListResult, SdkError> {
        Ok(match req.cursor.as_deref() {
            Some("topic:1") => crate::domain::TopicListResult {
                topics: vec![crate::domain::TopicRecord {
                    topic_id: crate::domain::TopicId("topic-2".to_owned()),
                    topic_path: Some(crate::domain::TopicPath("ops/secondary".to_owned())),
                    created_ts_ms: 701,
                    metadata: BTreeMap::new(),
                    extensions: BTreeMap::new(),
                }],
                next_cursor: None,
            },
            _ => crate::domain::TopicListResult {
                topics: vec![crate::domain::TopicRecord {
                    topic_id: crate::domain::TopicId("topic-1".to_owned()),
                    topic_path: Some(crate::domain::TopicPath("ops/alerts".to_owned())),
                    created_ts_ms: 700,
                    metadata: BTreeMap::from([("kind".to_owned(), serde_json::json!("ops"))]),
                    extensions: BTreeMap::new(),
                }],
                next_cursor: Some("topic:1".to_owned()),
            },
        })
    }

    fn topic_subscribe(
        &self,
        req: crate::domain::TopicSubscriptionRequest,
    ) -> Result<Ack, SdkError> {
        let _ = req;
        Ok(Ack { accepted: true, revision: None })
    }

    fn topic_unsubscribe(&self, _topic_id: crate::domain::TopicId) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: None })
    }

    fn topic_publish(&self, req: crate::domain::TopicPublishRequest) -> Result<Ack, SdkError> {
        let _ = req;
        Ok(Ack { accepted: true, revision: None })
    }

    fn telemetry_query(
        &self,
        query: crate::domain::TelemetryQuery,
    ) -> Result<Vec<crate::domain::TelemetryPoint>, SdkError> {
        Ok(vec![crate::domain::TelemetryPoint {
            ts_ms: query.from_ts_ms.unwrap_or(900),
            key: "topic_publish".to_owned(),
            value: serde_json::json!({ "message": "hello topic" }),
            unit: None,
            tags: BTreeMap::from([
                (
                    "topic_id".to_owned(),
                    query.topic_id.map(|value| value.0).unwrap_or_else(|| "topic-1".to_owned()),
                ),
                ("peer_id".to_owned(), query.peer_id.unwrap_or_else(|| "node-b".to_owned())),
            ]),
            extensions: query.extensions,
        }])
    }

    fn telemetry_subscribe(&self, _query: crate::domain::TelemetryQuery) -> Result<Ack, SdkError> {
        Ok(Ack { accepted: true, revision: None })
    }

    fn marker_create(
        &self,
        req: crate::domain::MarkerCreateRequest,
    ) -> Result<crate::domain::MarkerRecord, SdkError> {
        Ok(crate::domain::MarkerRecord {
            marker_id: crate::domain::MarkerId("marker-1".to_owned()),
            label: req.label,
            position: req.position,
            topic_id: req.topic_id,
            revision: 1,
            updated_ts_ms: 950,
            extensions: req.extensions,
        })
    }

    fn marker_list(
        &self,
        req: crate::domain::MarkerListRequest,
    ) -> Result<crate::domain::MarkerListResult, SdkError> {
        Ok(crate::domain::MarkerListResult {
            markers: vec![crate::domain::MarkerRecord {
                marker_id: crate::domain::MarkerId("marker-1".to_owned()),
                label: "Alpha".to_owned(),
                position: crate::domain::GeoPoint { lat: 35.0, lon: -115.0, alt_m: Some(1200.0) },
                topic_id: req.topic_id.or(Some(crate::domain::TopicId("topic-1".to_owned()))),
                revision: 2,
                updated_ts_ms: 960,
                extensions: BTreeMap::new(),
            }],
            next_cursor: None,
        })
    }

    fn marker_update_position(
        &self,
        req: crate::domain::MarkerUpdatePositionRequest,
    ) -> Result<crate::domain::MarkerRecord, SdkError> {
        Ok(crate::domain::MarkerRecord {
            marker_id: req.marker_id,
            label: "Alpha".to_owned(),
            position: req.position,
            topic_id: Some(crate::domain::TopicId("topic-1".to_owned())),
            revision: req.expected_revision.saturating_add(1),
            updated_ts_ms: 970,
            extensions: req.extensions,
        })
    }

    fn marker_delete(&self, req: crate::domain::MarkerDeleteRequest) -> Result<Ack, SdkError> {
        let _ = req;
        Ok(Ack { accepted: true, revision: None })
    }

    fn command_invoke(
        &self,
        req: crate::domain::RemoteCommandRequest,
    ) -> Result<crate::domain::RemoteCommandResponse, SdkError> {
        self.remote_command_results
            .lock()
            .expect("remote command results")
            .pop_front()
            .unwrap_or_else(|| {
                Ok(crate::domain::RemoteCommandResponse {
                    accepted: true,
                    payload: serde_json::json!({
                        "command": req.command,
                        "target": req.target,
                        "payload": req.payload,
                    }),
                    extensions: req.extensions,
                })
            })
    }

    fn envelope_execute(
        &self,
        envelope: crate::app::Envelope,
    ) -> Result<crate::app::EnvelopeResponse, SdkError> {
        self.envelope_results.lock().expect("envelope results").pop_front().unwrap_or_else(|| {
            Ok(crate::app::EnvelopeResponse {
                operation_id: envelope.operation_id,
                kind: crate::app::EnvelopeKind::Result,
                accepted: true,
                correlation_id: envelope.correlation_id,
                payload: serde_json::json!({
                    "query": true,
                    "payload": envelope.payload,
                }),
                extensions: envelope.extensions,
            })
        })
    }

    fn voice_session_open(
        &self,
        _req: crate::domain::VoiceSessionOpenRequest,
    ) -> Result<crate::domain::VoiceSessionId, SdkError> {
        self.voice_open_results
            .lock()
            .expect("voice open results")
            .pop_front()
            .unwrap_or_else(|| Ok(crate::domain::VoiceSessionId("voice-1".to_owned())))
    }

    fn voice_session_update(
        &self,
        _req: crate::domain::VoiceSessionUpdateRequest,
    ) -> Result<crate::domain::VoiceSessionState, SdkError> {
        self.voice_update_results
            .lock()
            .expect("voice update results")
            .pop_front()
            .unwrap_or(Ok(crate::domain::VoiceSessionState::Active))
    }

    fn voice_session_close(
        &self,
        _session_id: crate::domain::VoiceSessionId,
    ) -> Result<Ack, SdkError> {
        self.voice_close_results
            .lock()
            .expect("voice close results")
            .pop_front()
            .unwrap_or(Ok(Ack { accepted: true, revision: None }))
    }
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
        let events = self
            .live_events
            .lock()
            .expect("live events")
            .drain(..)
            .collect::<Vec<_>>();
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
        Box::pin(async move { self.negotiate(req) })
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

pub(super) fn runtime_started_event() -> SdkEvent {
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

pub(super) fn stream_gap_event() -> SdkEvent {
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
