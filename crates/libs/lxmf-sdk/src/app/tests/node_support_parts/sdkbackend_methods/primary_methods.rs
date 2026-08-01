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
                    peers: vec![eve.clone()],
                    next_cursor: Some("presence:1".to_owned()),
                },
                Some("presence:1") => {
                    crate::domain::PresenceListResult { peers: vec![bob], next_cursor: None }
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
