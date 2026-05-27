macro_rules! mock_backend_topic_marker_methods {
    () => {
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
    };
}
