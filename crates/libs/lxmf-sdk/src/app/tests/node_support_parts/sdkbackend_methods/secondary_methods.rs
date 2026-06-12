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
