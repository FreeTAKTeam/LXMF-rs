macro_rules! mock_backend_discovery_attachment_methods {
    () => {
    fn identity_list(&self) -> Result<Vec<crate::domain::IdentityBundle>, SdkError> {
        Ok(vec![crate::domain::IdentityBundle {
            identity: crate::domain::IdentityRef("alice".to_owned()),
            delivery_destination: None,
            public_key: "pubkey".to_owned(),
            display_name: Some("Alice".to_owned()),
            capabilities: vec!["chat".to_owned()],
            metadata: BTreeMap::new(),
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
    };
}
