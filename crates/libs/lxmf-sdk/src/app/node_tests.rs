use super::{Client, Envelope};
use crate::app::DeliveryOptions;
use crate::app::{
    Config, DeliveryState, EnvelopeKind, OperationEntry, OperationKind, Profile, RunState,
    SendRequest, SubscriptionStart, TransportVariant,
};
use crate::domain::TrustLevel;
use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};
use crate::event::{
    EventBatch as RawEventBatch, EventCursor, EventSubscription, SdkEvent, Severity as RawSeverity,
};
use crate::{
    Ack, CancelResult, DeliverySnapshot, DeliveryState as RawDeliveryState, EffectiveLimits,
    NegotiationRequest, NegotiationResponse, Profile as CoreProfile, RuntimeSnapshot, RuntimeState,
    SdkBackend, SdkBackendAsyncEvents, SendRequest as RawSendRequest, ShutdownMode,
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
    fn new() -> Self {
        Self {
            runtime_seq: AtomicUsize::new(1),
            send_seq: AtomicUsize::new(1),
            paginate_discovery: false,
            poll_batches: Mutex::new(VecDeque::new()),
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

    fn new_paginated() -> Self {
        Self { paginate_discovery: true, ..Self::new() }
    }

    fn queue_batch(&self, batch: RawEventBatch) {
        self.poll_batches.lock().expect("poll batches").push_back(batch);
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

#[test]
fn config_presets_map_to_expected_profiles() {
    assert_eq!(Config::mobile_default().profile, Profile::MobileDefault);
    assert_eq!(Config::mobile_default().sdk_config.profile, CoreProfile::DesktopLocalRuntime);
    assert_eq!(Config::desktop_default().sdk_config.profile, CoreProfile::DesktopFull);
    assert_eq!(Config::embedded_default().sdk_config.profile, CoreProfile::EmbeddedAlloc);
}

#[test]
fn config_operation_registry_merges_custom_entries() {
    let config = Config::testing_default().with_custom_operation(OperationEntry::new(
        "vendor.example.custom",
        "custom",
        OperationKind::Command,
        TransportVariant::Extension,
        "Custom vendor command.",
    ));
    let registry = config.operation_registry().expect("registry");
    assert!(registry.supports("vendor.example.custom"));
    assert!(registry.supports("sdk_poll_events_v2"));
}

#[test]
fn client_exposes_built_in_registry_before_start() {
    let app = Client::new(MockBackend::new());
    let registry = app.operation_registry().expect("registry");
    assert_eq!(
        registry.canonicalize("sdk_identity_contact_list_v2").expect("canonical id").as_str(),
        "app.contact.list"
    );
}

#[test]
fn execute_envelope_routes_runtime_status_locally() {
    let app = Client::new(MockBackend::new());
    let response = app.query("app.runtime.status", serde_json::json!({})).expect("runtime status");
    assert_eq!(response.kind, EnvelopeKind::Result);
    assert_eq!(response.operation_id.as_str(), "app.runtime.status");
    assert_eq!(response.payload.get("state").and_then(|value| value.as_str()), Some("new"));
}

#[test]
fn execute_envelope_routes_identity_queries_to_backend() {
    let app = Client::new(MockBackend::new());
    let response = app.query("app.identity.list", serde_json::json!({})).expect("identity list");
    let identities = response.payload.as_array().expect("identity array");
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].get("display_name").and_then(|value| value.as_str()), Some("Alice"));
}

#[test]
fn execute_envelope_accepts_registered_aliases() {
    let app = Client::new(MockBackend::new());
    let response =
        app.query("sdk_identity_list_v2", serde_json::json!({})).expect("identity list via alias");
    assert_eq!(response.operation_id.as_str(), "app.identity.list");
    let identities = response.payload.as_array().expect("identity array");
    assert_eq!(identities[0]["identity"], json!("alice"));
}

#[test]
fn execute_envelope_routes_discovery_operations_locally() {
    let app = Client::new(MockBackend::new());

    let announce =
        app.command("sdk_identity_announce_now_v2", serde_json::json!({})).expect("announce");
    assert_eq!(announce.operation_id.as_str(), "app.identity.announce");
    assert_eq!(announce.payload["accepted"], json!(true));

    let presence = app
        .query("sdk_identity_presence_list_v2", serde_json::json!({ "limit": 10 }))
        .expect("presence");
    assert_eq!(presence.operation_id.as_str(), "app.identity.presence.list");
    assert_eq!(presence.payload["peers"].as_array().expect("peer rows").len(), 2);

    let contact = app
        .command(
            "sdk_identity_contact_update_v2",
            serde_json::json!({
                "identity": "charlie",
                "display_name": "Charlie",
                "trust_level": "trusted",
                "bootstrap": true
            }),
        )
        .expect("contact update");
    assert_eq!(contact.operation_id.as_str(), "app.contact.update");
    assert_eq!(contact.payload["identity"], json!("charlie"));

    let bootstrap = app
        .command(
            "sdk_identity_bootstrap_v2",
            serde_json::json!({ "identity": "delta", "auto_sync": true }),
        )
        .expect("bootstrap");
    assert_eq!(bootstrap.operation_id.as_str(), "app.identity.bootstrap");
    assert_eq!(bootstrap.payload["identity"], json!("delta"));
}

#[test]
fn execute_envelope_routes_topic_operations_locally() {
    let app = Client::new(MockBackend::new());

    let topic = app
        .command(
            "sdk_topic_create_v2",
            serde_json::json!({
                "topic_path": "ops/alerts",
                "metadata": { "kind": "ops" }
            }),
        )
        .expect("topic create");
    assert_eq!(topic.operation_id.as_str(), "app.topic.create");
    assert_eq!(topic.payload["topic_id"], json!("topic-1"));

    let fetched = app.query("sdk_topic_get_v2", serde_json::json!("topic-1")).expect("topic get");
    assert_eq!(fetched.operation_id.as_str(), "app.topic.get");
    assert_eq!(fetched.payload["topic_path"], json!("ops/alerts"));

    let listed =
        app.query("app.topic.list", serde_json::json!({ "limit": 10 })).expect("topic list");
    assert_eq!(listed.payload["topics"].as_array().expect("topic list").len(), 1);
    assert_eq!(listed.payload["next_cursor"], json!("topic:1"));

    let subscribed = app
        .command("sdk_topic_subscribe_v2", serde_json::json!({ "topic_id": "topic-1" }))
        .expect("topic subscribe");
    assert_eq!(subscribed.operation_id.as_str(), "app.topic.subscribe");
    assert_eq!(subscribed.payload["accepted"], json!(true));

    let published = app
        .command(
            "app.topic.publish",
            serde_json::json!({
                "topic_id": "topic-1",
                "payload": { "message": "hello topic" },
                "correlation_id": "topic-corr-1"
            }),
        )
        .expect("topic publish");
    assert_eq!(published.operation_id.as_str(), "app.topic.publish");
    assert_eq!(published.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_workflow_operations_locally() {
    let app = Client::new(MockBackend::new());
    app.start(Config::testing_default()).expect("start");

    let peer_ready = app
        .command(
            "sdk_workflow_peer_ready_v2",
            serde_json::json!({
                "identity": "delta",
                "announce": true,
                "bootstrap": true,
            }),
        )
        .expect("workflow peer ready");
    assert_eq!(peer_ready.operation_id.as_str(), "app.workflow.peer_ready");
    assert_eq!(peer_ready.payload["contact"]["identity"], json!("delta"));

    let topic_sync = app
        .command(
            "sdk_workflow_topic_sync_v2",
            serde_json::json!({
                "topic_path": "ops/alerts",
                "telemetry_limit": 5,
            }),
        )
        .expect("workflow topic sync");
    assert_eq!(topic_sync.operation_id.as_str(), "app.workflow.topic_sync");
    assert_eq!(topic_sync.payload["topic"]["topic_id"], json!("topic-1"));
    assert_eq!(topic_sync.payload["subscribed"], json!(true));

    let mission = app
        .command(
            "sdk_workflow_mission_update_send_v2",
            serde_json::json!({
                "peer_identity": "delta",
                "content": "mission update",
                "topic_path": "ops/alerts",
                "attachments": [{
                    "name": "sitrep.txt",
                    "content_type": "text/plain",
                    "bytes_base64": "c2l0cmVw",
                }],
            }),
        )
        .expect("workflow mission update");
    assert_eq!(mission.operation_id.as_str(), "app.workflow.mission_update_send");
    assert_eq!(mission.payload["message_id"], json!("msg-1"));
    assert_eq!(mission.payload["attachments"].as_array().expect("attachments").len(), 1);
}

#[test]
fn execute_envelope_rejects_reserved_mission_metadata() {
    let app = Client::new(MockBackend::new());

    let err = app.command(
        "sdk_workflow_mission_update_send_v2",
        serde_json::json!({
            "peer_identity": "delta",
            "content": "mission update",
            "metadata": {
                "topic_id": "override",
            },
        }),
    );
    assert!(err.is_err());
}

#[test]
fn execute_envelope_routes_telemetry_operations_locally() {
    let app = Client::new(MockBackend::new());

    let telemetry = app
        .query(
            "sdk_telemetry_query_v2",
            serde_json::json!({
                "topic_id": "topic-1",
                "peer_id": "node-b",
                "from_ts_ms": 100,
                "limit": 10,
            }),
        )
        .expect("telemetry query");
    assert_eq!(telemetry.operation_id.as_str(), "app.telemetry.query");
    assert_eq!(telemetry.payload.as_array().expect("telemetry rows").len(), 1);
    assert_eq!(telemetry.payload[0]["tags"]["topic_id"], json!("topic-1"));

    let subscribed = app
        .command(
            "app.telemetry.subscribe",
            serde_json::json!({
                "topic_id": "topic-1",
                "from_ts_ms": 100,
                "limit": 20,
            }),
        )
        .expect("telemetry subscribe");
    assert_eq!(subscribed.operation_id.as_str(), "app.telemetry.subscribe");
    assert_eq!(subscribed.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_attachment_operations_locally() {
    let app = Client::new(MockBackend::new());

    let stored = app
        .command(
            "sdk_attachment_store_v2",
            serde_json::json!({
                "name": "sample.txt",
                "content_type": "text/plain",
                "bytes_base64": "aGVsbG8gd29ybGQ=",
                "topic_ids": ["topic-1"],
            }),
        )
        .expect("attachment store");
    assert_eq!(stored.operation_id.as_str(), "app.attachment.store");
    assert_eq!(stored.payload["attachment_id"], json!("attachment-1"));

    let fetched = app
        .query("sdk_attachment_get_v2", serde_json::json!("attachment-1"))
        .expect("attachment get");
    assert_eq!(fetched.operation_id.as_str(), "app.attachment.get");
    assert_eq!(fetched.payload["name"], json!("sample.txt"));

    let listed = app
        .query(
            "app.attachment.list",
            serde_json::json!({
                "topic_id": "topic-1",
                "limit": 10,
            }),
        )
        .expect("attachment list");
    assert_eq!(listed.payload["attachments"].as_array().expect("attachment rows").len(), 1);

    let associated = app
        .command(
            "sdk_attachment_associate_topic_v2",
            serde_json::json!({
                "attachment_id": "attachment-1",
                "topic_id": "topic-2",
            }),
        )
        .expect("attachment associate");
    assert_eq!(associated.operation_id.as_str(), "app.attachment.associate_topic");
    assert_eq!(associated.payload["accepted"], json!(true));

    let deleted = app
        .command("app.attachment.delete", serde_json::json!("attachment-1"))
        .expect("attachment delete");
    assert_eq!(deleted.operation_id.as_str(), "app.attachment.delete");
    assert_eq!(deleted.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_attachment_streaming_operations_locally() {
    let app = Client::new(MockBackend::new());

    let upload = app
            .command(
                "sdk_attachment_upload_start_v2",
                serde_json::json!({
                    "name": "chunked.bin",
                    "content_type": "application/octet-stream",
                    "total_size": 11,
                    "checksum_sha256": "64ec88ca00b268e5ba1a35678a1b5316d212f4f366b2477232534a8aeca37f3c",
                    "topic_ids": ["topic-1"],
                }),
            )
            .expect("attachment upload start");
    assert_eq!(upload.operation_id.as_str(), "app.attachment.upload_start");
    assert_eq!(upload.payload["upload_id"], json!("upload-1"));

    let chunk = app
        .command(
            "app.attachment.upload_chunk",
            serde_json::json!({
                "upload_id": "upload-1",
                "offset": 0,
                "bytes_base64": "aGVsbG8=",
            }),
        )
        .expect("attachment upload chunk");
    assert_eq!(chunk.operation_id.as_str(), "app.attachment.upload_chunk");
    assert_eq!(chunk.payload["accepted"], json!(true));

    let committed = app
        .command(
            "sdk_attachment_upload_commit_v2",
            serde_json::json!({
                "upload_id": "upload-1",
            }),
        )
        .expect("attachment upload commit");
    assert_eq!(committed.operation_id.as_str(), "app.attachment.upload_commit");
    assert_eq!(committed.payload["attachment_id"], json!("attachment-1"));

    let downloaded = app
        .query(
            "sdk_attachment_download_chunk_v2",
            serde_json::json!({
                "attachment_id": "attachment-1",
                "offset": 0,
                "max_bytes": 5,
            }),
        )
        .expect("attachment download chunk");
    assert_eq!(downloaded.operation_id.as_str(), "app.attachment.download_chunk");
    assert_eq!(downloaded.payload["next_offset"], json!(5));
    assert_eq!(downloaded.payload["done"], json!(false));
}

#[test]
fn execute_envelope_routes_marker_operations_locally() {
    let app = Client::new(MockBackend::new());

    let created = app
        .command(
            "sdk_marker_create_v2",
            serde_json::json!({
                "label": "Alpha",
                "position": { "lat": 35.0, "lon": -115.0, "alt_m": 1200.0 },
                "topic_id": "topic-1",
            }),
        )
        .expect("marker create");
    assert_eq!(created.operation_id.as_str(), "app.marker.create");
    assert_eq!(created.payload["marker_id"], json!("marker-1"));

    let listed = app
        .query(
            "app.marker.list",
            serde_json::json!({
                "topic_id": "topic-1",
                "limit": 10,
            }),
        )
        .expect("marker list");
    assert_eq!(listed.payload["markers"].as_array().expect("marker rows").len(), 1);

    let updated = app
        .command(
            "sdk_marker_update_position_v2",
            serde_json::json!({
                "marker_id": "marker-1",
                "expected_revision": 2,
                "position": { "lat": 36.0, "lon": -116.0, "alt_m": null },
            }),
        )
        .expect("marker update");
    assert_eq!(updated.operation_id.as_str(), "app.marker.update_position");
    assert_eq!(updated.payload["revision"], json!(3));

    let deleted = app
        .command(
            "app.marker.delete",
            serde_json::json!({
                "marker_id": "marker-1",
                "expected_revision": 3,
            }),
        )
        .expect("marker delete");
    assert_eq!(deleted.operation_id.as_str(), "app.marker.delete");
    assert_eq!(deleted.payload["accepted"], json!(true));
}

#[test]
fn execute_envelope_routes_runtime_start_and_stop_locally() {
    let app = Client::new(MockBackend::new());
    let start = app
        .command(
            "app.runtime.start",
            serde_json::to_value(Config::testing_default()).expect("config value"),
        )
        .expect("runtime start");
    assert_eq!(start.operation_id.as_str(), "app.runtime.start");
    assert_eq!(
        start.payload.get("profile").and_then(|value| value.as_str()),
        Some("testing_default")
    );

    let stop = app
        .command("app.runtime.stop", serde_json::json!({ "mode": "graceful" }))
        .expect("runtime stop");
    assert_eq!(stop.operation_id.as_str(), "app.runtime.stop");
    assert_eq!(stop.payload.get("accepted").and_then(|value| value.as_bool()), Some(true));
}

#[test]
fn execute_envelope_routes_delivery_send_locally() {
    let backend = MockBackend::new();
    backend.queue_send_result(Ok(crate::MessageId("msg-1".to_owned())));
    let app = Client::new(backend);
    app.start(Config::testing_default()).expect("start");

    let response = app
        .command(
            "app.delivery.send",
            serde_json::json!({
                "source": "src",
                "destination": "dst",
                "payload": { "content": "hello" },
                "correlation_id": "corr-1"
            }),
        )
        .expect("delivery send");
    assert_eq!(response.operation_id.as_str(), "app.delivery.send");
    assert_eq!(response.payload.get("message_id").and_then(|value| value.as_str()), Some("msg-1"));
}

#[test]
fn execute_envelope_routes_custom_commands_via_remote_command_backend() {
    let backend = MockBackend::new();
    backend.queue_remote_command_result(Ok(crate::domain::RemoteCommandResponse {
        accepted: true,
        payload: serde_json::json!({
            "command_id": "cmdreq-1",
            "correlation_id": "cmd-1",
            "command": "vendor.example.custom",
            "target": null,
            "command_state": "dispatched",
        }),
        extensions: BTreeMap::from([("transport".to_owned(), serde_json::json!("remote"))]),
    }));
    let app = Client::new(backend);
    app.start(Config::desktop_default().with_custom_operation(OperationEntry::new(
        "vendor.example.custom",
        "custom",
        OperationKind::Command,
        TransportVariant::Extension,
        "Custom vendor command.",
    )))
    .expect("start");
    let response = app
        .command("vendor.example.custom", serde_json::json!({ "value": 1 }))
        .expect("custom command");
    assert_eq!(response.operation_id.as_str(), "vendor.example.custom");
    assert_eq!(
        response.payload.get("command_state").and_then(|value| value.as_str()),
        Some("dispatched")
    );
    assert_eq!(
        response.extensions.get("transport").and_then(|value| value.as_str()),
        Some("remote")
    );
}

#[test]
fn execute_envelope_rejects_kind_mismatches() {
    let app = Client::new(MockBackend::new());
    let err = app
        .execute_envelope(Envelope::command("app.identity.list", serde_json::json!({})))
        .expect_err("kind mismatch should fail");
    assert_eq!(err.code.as_str(), "SDK_APP_VALIDATION_INVALID_ARGUMENT");
}

#[test]
fn execute_envelope_routes_unhandled_queries_to_backend_envelope_path() {
    let backend = MockBackend::new();
    backend.queue_envelope_result(Ok(crate::app::EnvelopeResponse {
        operation_id: crate::app::OperationId::from("app.message.history.list"),
        kind: crate::app::EnvelopeKind::Result,
        accepted: true,
        correlation_id: Some("corr-1".to_owned()),
        payload: serde_json::json!({ "messages": [] }),
        extensions: BTreeMap::from([("via".to_owned(), serde_json::json!("envelope"))]),
    }));
    let app = Client::new(backend);
    let response = app
        .query("app.message.history.list", serde_json::json!({ "limit": 10 }))
        .expect("history query");
    assert_eq!(response.operation_id.as_str(), "app.message.history.list");
    assert_eq!(response.extensions.get("via").and_then(|value| value.as_str()), Some("envelope"));
}

#[test]
fn execute_envelope_routes_voice_operations_locally() {
    let backend = MockBackend::new();
    backend.queue_voice_open_result(Ok(crate::domain::VoiceSessionId("voice-9".to_owned())));
    backend.queue_voice_update_result(Ok(crate::domain::VoiceSessionState::Active));
    backend.queue_voice_close_result(Ok(Ack { accepted: true, revision: None }));
    let app = Client::new(backend);

    let opened = app
        .command(
            "app.voice.session.open",
            serde_json::json!({ "peer_id": "node-b", "codec_hint": "opus" }),
        )
        .expect("voice open");
    assert_eq!(opened.operation_id.as_str(), "app.voice.session.open");
    assert_eq!(
        serde_json::from_value::<crate::domain::VoiceSessionId>(opened.payload).expect("voice id"),
        crate::domain::VoiceSessionId("voice-9".to_owned())
    );

    let updated = app
        .command(
            "app.voice.session.update",
            serde_json::json!({ "session_id": "voice-9", "state": "active" }),
        )
        .expect("voice update");
    assert_eq!(updated.operation_id.as_str(), "app.voice.session.update");
    assert_eq!(
        serde_json::from_value::<crate::domain::VoiceSessionState>(updated.payload)
            .expect("voice state"),
        crate::domain::VoiceSessionState::Active
    );

    let closed =
        app.command("app.voice.session.close", serde_json::json!("voice-9")).expect("voice close");
    assert_eq!(closed.operation_id.as_str(), "app.voice.session.close");
    assert_eq!(closed.payload.get("accepted").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(closed.payload.get("session_id").and_then(|value| value.as_str()), Some("voice-9"));
}

#[test]
fn discovery_helpers_map_backend_identity_contact_and_presence_models() {
    let app = Client::new(MockBackend::new());

    let identities = app.identities().expect("identities");
    assert_eq!(identities.len(), 1);
    assert_eq!(identities[0].identity, "alice");
    assert_eq!(identities[0].display_name.as_deref(), Some("Alice"));

    let contacts = app.contacts(Some("cursor-1".to_owned()), Some(5)).expect("contacts");
    assert_eq!(contacts.contacts.len(), 1);
    assert_eq!(contacts.contacts[0].identity, "bob");
    assert_eq!(contacts.contacts[0].trust_level, TrustLevel::Trusted);
    assert_eq!(
        contacts.contacts[0].extensions.get("cursor").and_then(|value| value.as_str()),
        Some("cursor-1")
    );

    let presence = app.presence(None, Some(10)).expect("presence");
    assert_eq!(presence.peers.len(), 2);
    assert_eq!(presence.peers[0].peer_id, "bob");
    assert_eq!(presence.peers[0].display_name.as_deref(), Some("Bob Relay"));
    assert_eq!(presence.peers[0].trust_level, Some(TrustLevel::Trusted));
    assert!(presence.peers[0].bootstrap.unwrap_or(false));
}

#[test]
fn discovery_helpers_update_contacts_and_bootstrap_identities() {
    let app = Client::new(MockBackend::new());

    let updated = app
        .update_contact(
            super::super::discovery::ContactUpdate::new("charlie")
                .with_display_name("Charlie")
                .with_trust_level(TrustLevel::Untrusted)
                .with_bootstrap(true),
        )
        .expect("contact update");
    assert_eq!(updated.identity, "charlie");
    assert_eq!(updated.display_name.as_deref(), Some("Charlie"));
    assert_eq!(updated.trust_level, TrustLevel::Untrusted);
    assert!(updated.bootstrap);

    let bootstrapped = app
        .bootstrap_identity(super::super::discovery::BootstrapRequest::new("delta"))
        .expect("bootstrap");
    assert_eq!(bootstrapped.identity, "delta");
    assert_eq!(bootstrapped.trust_level, TrustLevel::Trusted);
    assert!(bootstrapped.bootstrap);
}

#[test]
fn peer_directory_merges_contact_and_presence_views() {
    let app = Client::new(MockBackend::new());
    let peers = app.peer_directory(Some(10)).expect("peer directory");

    assert_eq!(peers.len(), 2);

    let bob = peers.iter().find(|entry| entry.peer_id == "bob").expect("bob entry");
    assert_eq!(bob.display_name.as_deref(), Some("Bob"));
    assert_eq!(bob.name_source.as_deref(), Some("contact"));
    assert_eq!(bob.trust_level, Some(TrustLevel::Trusted));
    assert!(bob.online);
    assert!(bob.bootstrap);
    assert_eq!(bob.last_seen_ts_ms, Some(200));
    assert_eq!(bob.first_seen_ts_ms, Some(120));
    assert_eq!(bob.seen_count, 3);

    let eve = peers.iter().find(|entry| entry.peer_id == "eve").expect("eve entry");
    assert_eq!(eve.display_name.as_deref(), Some("Eve"));
    assert_eq!(eve.name_source.as_deref(), Some("announce"));
    assert_eq!(eve.trust_level, Some(TrustLevel::Unknown));
    assert!(eve.online);
    assert!(!eve.bootstrap);
}

#[test]
fn peer_directory_consumes_all_contact_and_presence_pages() {
    let app = Client::new(MockBackend::new_paginated());
    let peers = app.peer_directory(None).expect("peer directory");

    assert_eq!(peers.len(), 3);
    assert!(peers.iter().any(|entry| entry.peer_id == "bob"));
    assert!(peers.iter().any(|entry| entry.peer_id == "charlie"));
    assert!(peers.iter().any(|entry| entry.peer_id == "eve"));
}

#[test]
fn client_restarts_by_recreating_inner_client() {
    let backend = MockBackend::new();
    let app = Client::new(backend);
    let first = app.start(Config::desktop_default()).expect("first start");
    app.stop(ShutdownMode::Immediate).expect("stop");
    let second = app.start(Config::desktop_default()).expect("second start");
    assert_ne!(first.runtime_id, second.runtime_id);
}

#[test]
fn client_send_and_status_hide_raw_sdk_types() {
    let backend = MockBackend::new();
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");
    let receipt = app
        .send(
            SendRequest::new("src", "dst", json!({ "body": "hello" }))
                .with_correlation_id("corr-1"),
        )
        .expect("send");
    assert_eq!(receipt.profile, Profile::DesktopDefault);
    assert_eq!(receipt.correlation_id.as_deref(), Some("corr-1"));

    let status = app
        .delivery_status(receipt.message_id.as_str())
        .expect("delivery status")
        .expect("snapshot");
    assert_eq!(status.state, DeliveryState::Sent);
}

#[test]
fn client_status_reports_degraded_after_gap_event() {
    let backend = MockBackend::new();
    backend.queue_batch(RawEventBatch {
        events: vec![runtime_started_event(), stream_gap_event()],
        next_cursor: EventCursor("cursor-2".to_owned()),
        dropped_count: 3,
        snapshot_high_watermark_seq_no: None,
        extensions: BTreeMap::new(),
    });

    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");
    let mut stream = app.subscribe_events(SubscriptionStart::Head).expect("subscribe");
    let batch = stream.next_batch().expect("next batch");
    assert_eq!(batch.events.len(), 2);

    let status = app.status().expect("status");
    assert_eq!(status.state, RunState::Degraded);
}

#[test]
fn client_returns_not_started_before_start() {
    let app = Client::new(MockBackend::new());
    let err = app
        .send(SendRequest::new("src", "dst", json!({ "body": "hello" })))
        .expect_err("send should fail");
    assert_eq!(err.code.as_str(), "SDK_APP_RUNTIME_NOT_STARTED");
    assert!(!err.user_action_required);
}

#[test]
fn failed_stop_preserves_live_session_state() {
    let backend = MockBackend::new();
    backend.queue_shutdown_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "shutdown failed",
    )));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let err = app.stop(ShutdownMode::Immediate).expect_err("stop should fail");
    assert_eq!(err.code.as_str(), "SDK_APP_INTERNAL_UNEXPECTED_FAILURE");

    let receipt = app
        .send(SendRequest::new("src", "dst", json!({ "body": "still-live" })))
        .expect("send after failed stop");
    assert_eq!(receipt.profile, Profile::DesktopDefault);
}

#[test]
fn restart_propagates_stop_failures() {
    let backend = MockBackend::new();
    backend.queue_shutdown_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "shutdown failed",
    )));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let err =
        app.restart(Config::desktop_default()).expect_err("restart should fail when stop fails");
    assert_eq!(err.code.as_str(), "SDK_APP_INTERNAL_UNEXPECTED_FAILURE");
}

#[test]
fn delivery_plan_tracks_profile_defaults() {
    let config = Config::desktop_default();
    let plan = config.delivery_plan();

    assert_eq!(plan.profile, Profile::DesktopDefault);
    assert_eq!(plan.retry.max_attempts, 5);
    assert!(plan.reconnect.enabled);
    assert_eq!(plan.default_event_batch_size, 64);
    assert!(plan.redaction_enabled);
}

#[test]
fn send_with_profile_defaults_retries_queue_pressure() {
    let backend = MockBackend::new();
    backend.queue_send_result(Err(SdkError::new(
        "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
        SdkErrorCategory::Runtime,
        "full",
    )
    .with_retryable(true)));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let report = app
        .send_with_profile_defaults(SendRequest::new("src", "dst", json!({ "body": "hello" })))
        .expect("report");

    assert_eq!(report.attempts.len(), 1);
    assert_eq!(report.attempts[0].disposition, super::super::delivery::AttemptDisposition::Retried);
    assert!(report.attempts[0].queue_pressure);
    assert_eq!(report.receipt.profile, Profile::DesktopDefault);
}

#[test]
fn send_with_options_can_fail_fast_on_queue_pressure() {
    let backend = MockBackend::new();
    backend.queue_send_result(Err(SdkError::new(
        "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
        SdkErrorCategory::Runtime,
        "full",
    )
    .with_retryable(true)));
    let app = Client::new(backend);
    app.start(Config::desktop_default()).expect("start");

    let err = app
        .send_with_options(
            SendRequest::new("src", "dst", json!({ "body": "hello" })),
            super::super::delivery::DeliveryOptions {
                queue_pressure_strategy: Some(
                    super::super::delivery::QueuePressureStrategy::FailFast,
                ),
                ..Default::default()
            },
        )
        .expect_err("queue pressure should fail fast");

    assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_QUEUE_PRESSURE");
}

#[test]
fn send_with_options_maps_retry_exhaustion() {
    let backend = MockBackend::new();
    backend.queue_send_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "temporary",
    )
    .with_retryable(true)));
    backend.queue_send_result(Err(SdkError::new(
        code::INTERNAL,
        SdkErrorCategory::Internal,
        "temporary",
    )
    .with_retryable(true)));
    let app = Client::new(backend);
    app.start(Config::testing_default()).expect("start");

    let err = app
        .send_with_options(
            SendRequest::new("src", "dst", json!({ "body": "hello" })),
            DeliveryOptions { max_attempts: Some(2), ..Default::default() },
        )
        .expect_err("retry exhaustion");

    assert_eq!(err.code.as_str(), "SDK_APP_DELIVERY_RETRY_EXHAUSTED");
    assert_eq!(err.cause_code.as_deref(), Some("SDK_INTERNAL_ERROR"));
}
