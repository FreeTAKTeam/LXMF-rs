macro_rules! mock_backend_core_methods {
    () => {
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
            contract_release: "v2.6".to_owned(),
            schema_namespace: "v2".to_owned(),
            sdk_version: crate::SDK_VERSION.to_owned(),
            python_reference: crate::ParityReference::default(),
            software_parity: None,
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
    };
}
