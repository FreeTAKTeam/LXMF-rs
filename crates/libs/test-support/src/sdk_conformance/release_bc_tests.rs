use super::{base_start_request, send_request, RpcHarness};
use base64::Engine as _;
use lxmf_sdk::{
    domain::{
        AttachmentDownloadChunkRequest, AttachmentStoreRequest, AttachmentUploadChunkRequest,
        AttachmentUploadCommitRequest, AttachmentUploadStartRequest, ContactListRequest,
        ContactUpdateRequest, GeoPoint, IdentityBootstrapRequest, IdentityImportRequest,
        IdentityResolveRequest, MarkerCreateRequest, MarkerDeleteRequest, MarkerListRequest,
        MarkerUpdatePositionRequest, PaperMessageEnvelope, PresenceListRequest,
        RemoteCommandRequest, RemoteCommandResponse, TelemetryQuery, TopicCreateRequest,
        TopicListRequest, TopicPath, TopicPublishRequest, TrustLevel, VoiceSessionOpenRequest,
        VoiceSessionState, VoiceSessionUpdateRequest,
    },
    LxmfSdk, LxmfSdkAttachments, LxmfSdkIdentity, LxmfSdkMarkers, LxmfSdkPaper,
    LxmfSdkRemoteCommands, LxmfSdkTelemetry, LxmfSdkTopics, LxmfSdkVoiceSignaling,
};
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;

#[test]
fn sdk_conformance_release_bc_domain_methods_work_through_rpc_adapter() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let topic = client
        .topic_create(TopicCreateRequest {
            topic_path: Some(TopicPath("ops/alerts".to_string())),
            metadata: BTreeMap::new(),
            extensions: BTreeMap::new(),
        })
        .expect("topic_create");
    let listed = client
        .topic_list(TopicListRequest { cursor: None, limit: Some(16), extensions: BTreeMap::new() })
        .expect("topic_list");
    assert!(
        listed.topics.iter().any(|record| record.topic_id == topic.topic_id),
        "created topic must appear in list"
    );

    client
        .topic_publish(TopicPublishRequest {
            topic_id: topic.topic_id.clone(),
            payload: json!({ "kind": "alert", "msg": "hello" }),
            correlation_id: Some("corr-1".to_string()),
            extensions: BTreeMap::new(),
        })
        .expect("topic_publish");
    let telemetry = client
        .telemetry_query(TelemetryQuery {
            peer_id: None,
            topic_id: Some(topic.topic_id.clone()),
            from_ts_ms: None,
            to_ts_ms: None,
            limit: Some(32),
            extensions: BTreeMap::new(),
        })
        .expect("telemetry_query");
    assert!(!telemetry.is_empty(), "topic publish should produce telemetry");

    let attachment = client
        .attachment_store(AttachmentStoreRequest {
            name: "note.txt".to_string(),
            content_type: "text/plain".to_string(),
            bytes_base64: "aGVsbG8=".to_string(),
            expires_ts_ms: None,
            topic_ids: vec![topic.topic_id.clone()],
            extensions: BTreeMap::new(),
        })
        .expect("attachment_store");
    let fetched_attachment = client
        .attachment_get(attachment.attachment_id.clone())
        .expect("attachment_get")
        .expect("attachment exists");
    assert_eq!(fetched_attachment.name, "note.txt");

    let chunk_payload = b"streaming-attachment".to_vec();
    let chunk_upload = client
        .attachment_upload_start(AttachmentUploadStartRequest {
            name: "stream.bin".to_string(),
            content_type: "application/octet-stream".to_string(),
            total_size: chunk_payload.len() as u64,
            checksum_sha256: "f66703569b2656c9eab514d35a87341514b0f489e7015bff6b0f7c460e89d104"
                .to_string(),
            expires_ts_ms: None,
            topic_ids: vec![topic.topic_id.clone()],
            extensions: BTreeMap::new(),
        })
        .expect("attachment_upload_start");
    let first = &chunk_payload[..6];
    let second = &chunk_payload[6..];
    let chunk_ack_1 = client
        .attachment_upload_chunk(AttachmentUploadChunkRequest {
            upload_id: chunk_upload.upload_id.clone(),
            offset: 0,
            bytes_base64: base64::engine::general_purpose::STANDARD.encode(first),
            extensions: BTreeMap::new(),
        })
        .expect("attachment_upload_chunk 1");
    assert_eq!(chunk_ack_1.next_offset, 6);
    let chunk_ack_2 = client
        .attachment_upload_chunk(AttachmentUploadChunkRequest {
            upload_id: chunk_upload.upload_id.clone(),
            offset: 6,
            bytes_base64: base64::engine::general_purpose::STANDARD.encode(second),
            extensions: BTreeMap::new(),
        })
        .expect("attachment_upload_chunk 2");
    assert!(chunk_ack_2.complete);
    let chunk_attachment = client
        .attachment_upload_commit(AttachmentUploadCommitRequest {
            upload_id: chunk_upload.upload_id.clone(),
            extensions: BTreeMap::new(),
        })
        .expect("attachment_upload_commit");
    let downloaded_chunk = client
        .attachment_download_chunk(AttachmentDownloadChunkRequest {
            attachment_id: chunk_attachment.attachment_id,
            offset: 6,
            max_bytes: 64,
            extensions: BTreeMap::new(),
        })
        .expect("attachment_download_chunk");
    assert_eq!(downloaded_chunk.offset, 6);
    assert!(downloaded_chunk.done);
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(downloaded_chunk.bytes_base64.as_bytes())
            .expect("decode downloaded chunk"),
        second
    );

    let marker = client
        .marker_create(MarkerCreateRequest {
            label: "alpha".to_string(),
            position: GeoPoint { lat: 34.0, lon: -118.0, alt_m: Some(100.0) },
            topic_id: Some(topic.topic_id.clone()),
            extensions: BTreeMap::new(),
        })
        .expect("marker_create");
    let marker_list = client
        .marker_list(MarkerListRequest {
            topic_id: Some(topic.topic_id.clone()),
            cursor: None,
            limit: Some(16),
            extensions: BTreeMap::new(),
        })
        .expect("marker_list");
    assert!(
        marker_list.markers.iter().any(|record| record.marker_id == marker.marker_id),
        "created marker must appear in list"
    );
    let updated_marker = client
        .marker_update_position(MarkerUpdatePositionRequest {
            marker_id: marker.marker_id.clone(),
            expected_revision: marker.revision,
            position: GeoPoint { lat: 35.0, lon: -117.0, alt_m: None },
            extensions: BTreeMap::new(),
        })
        .expect("marker_update_position");
    assert_eq!(updated_marker.position.lat, 35.0);
    client
        .marker_delete(MarkerDeleteRequest {
            marker_id: marker.marker_id,
            expected_revision: updated_marker.revision,
            extensions: BTreeMap::new(),
        })
        .expect("marker_delete");

    let identities = client.identity_list().expect("identity_list");
    assert!(!identities.is_empty(), "default identity expected");
    let imported = client
        .identity_import(IdentityImportRequest {
            bundle_base64: "eyJpZGVudGl0eSI6Im5vZGUtYiIsInB1YmxpY19rZXkiOiJub2RlLWItcHViIiwiZGlzcGxheV9uYW1lIjoiTm9kZSBCIiwiY2FwYWJpbGl0aWVzIjpbXSwiZXh0ZW5zaW9ucyI6e319".to_string(),
            passphrase: None,
            extensions: BTreeMap::new(),
        })
        .expect("identity_import");
    let resolved = client
        .identity_resolve(IdentityResolveRequest {
            hash: imported.public_key.clone(),
            extensions: BTreeMap::new(),
        })
        .expect("identity_resolve");
    assert!(resolved.is_some(), "imported identity should resolve by public key");
    let contact = client
        .identity_contact_update(ContactUpdateRequest {
            identity: imported.identity.clone(),
            display_name: Some("Node B Contact".to_string()),
            trust_level: Some(TrustLevel::Untrusted),
            bootstrap: Some(false),
            metadata: BTreeMap::new(),
            extensions: BTreeMap::new(),
        })
        .expect("identity_contact_update");
    assert_eq!(contact.trust_level, TrustLevel::Untrusted);
    let contacts = client
        .identity_contact_list(ContactListRequest {
            cursor: None,
            limit: Some(16),
            extensions: BTreeMap::new(),
        })
        .expect("identity_contact_list");
    assert!(
        contacts.contacts.iter().any(|row| row.identity == imported.identity),
        "contact list should include updated contact"
    );
    let bootstrapped = client
        .identity_bootstrap(IdentityBootstrapRequest {
            identity: imported.identity.clone(),
            auto_sync: true,
            extensions: BTreeMap::new(),
        })
        .expect("identity_bootstrap");
    assert_eq!(bootstrapped.trust_level, TrustLevel::Trusted);
    assert!(bootstrapped.bootstrap);
    let presence = client
        .identity_presence_list(PresenceListRequest {
            cursor: None,
            limit: Some(16),
            extensions: BTreeMap::new(),
        })
        .expect("identity_presence_list");
    assert!(
        presence.peers.iter().any(|row| {
            row.peer_id == imported.identity.0 && row.trust_level == Some(TrustLevel::Trusted)
        }),
        "presence list should include bootstrapped identity with trusted state"
    );
    assert!(client.identity_announce_now().expect("identity_announce_now").accepted);

    let sent = client.send(send_request("paper", None)).expect("send");
    let envelope = client.paper_encode(sent.clone()).expect("paper_encode");
    client
        .paper_decode(PaperMessageEnvelope {
            uri: envelope.uri,
            transient_id: envelope.transient_id,
            destination_hint: envelope.destination_hint,
            extensions: BTreeMap::new(),
        })
        .expect("paper_decode");

    let command_response = client
        .command_invoke(RemoteCommandRequest {
            command: "ping".to_string(),
            target: Some("node-b".to_string()),
            payload: json!({ "body": "hello" }),
            timeout_ms: Some(1_000),
            extensions: BTreeMap::new(),
        })
        .expect("command_invoke");
    let correlation_id = command_response
        .payload
        .get("correlation_id")
        .and_then(JsonValue::as_str)
        .expect("command response correlation_id")
        .to_string();
    client
        .command_reply(
            correlation_id,
            RemoteCommandResponse {
                accepted: true,
                payload: json!({ "body": "pong" }),
                extensions: BTreeMap::new(),
            },
        )
        .expect("command_reply");

    let voice_session = client
        .voice_session_open(VoiceSessionOpenRequest {
            peer_id: "node-b".to_string(),
            codec_hint: Some("opus".to_string()),
            extensions: BTreeMap::new(),
        })
        .expect("voice_session_open");
    let state = client
        .voice_session_update(VoiceSessionUpdateRequest {
            session_id: voice_session.clone(),
            state: VoiceSessionState::Active,
            extensions: BTreeMap::new(),
        })
        .expect("voice_session_update");
    assert_eq!(state, VoiceSessionState::Active);
    client.voice_session_close(voice_session).expect("voice_session_close");
}
