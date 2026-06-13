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
