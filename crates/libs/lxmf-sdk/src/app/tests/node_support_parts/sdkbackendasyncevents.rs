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
