use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TopicId(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TopicPath(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicCreateRequest {
    pub topic_path: Option<TopicPath>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicRecord {
    pub topic_id: TopicId,
    pub topic_path: Option<TopicPath>,
    pub created_ts_ms: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicPublishRequest {
    pub topic_id: TopicId,
    pub payload: JsonValue,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicListRequest {
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicListResult {
    pub topics: Vec<TopicRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopicSubscriptionRequest {
    pub topic_id: TopicId,
    pub cursor: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TelemetryQuery {
    pub peer_id: Option<String>,
    pub topic_id: Option<TopicId>,
    pub from_ts_ms: Option<u64>,
    pub to_ts_ms: Option<u64>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TelemetryPoint {
    pub ts_ms: u64,
    pub key: String,
    pub value: JsonValue,
    pub unit: Option<String>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AttachmentId(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentStoreRequest {
    pub name: String,
    pub content_type: String,
    pub bytes_base64: String,
    pub expires_ts_ms: Option<u64>,
    #[serde(default)]
    pub topic_ids: Vec<TopicId>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentMeta {
    pub attachment_id: AttachmentId,
    pub name: String,
    pub content_type: String,
    pub byte_len: u64,
    pub checksum_sha256: String,
    pub created_ts_ms: u64,
    pub expires_ts_ms: Option<u64>,
    #[serde(default)]
    pub topic_ids: Vec<TopicId>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentListRequest {
    pub topic_id: Option<TopicId>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentListResult {
    pub attachments: Vec<AttachmentMeta>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AttachmentUploadId(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentUploadStartRequest {
    pub name: String,
    pub content_type: String,
    pub total_size: u64,
    pub checksum_sha256: String,
    pub expires_ts_ms: Option<u64>,
    #[serde(default)]
    pub topic_ids: Vec<TopicId>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentUploadSession {
    pub upload_id: AttachmentUploadId,
    pub attachment_id: AttachmentId,
    pub chunk_size_hint: usize,
    pub next_offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentUploadChunkRequest {
    pub upload_id: AttachmentUploadId,
    pub offset: u64,
    pub bytes_base64: String,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentUploadChunkAck {
    pub accepted: bool,
    pub next_offset: u64,
    pub complete: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentUploadCommitRequest {
    pub upload_id: AttachmentUploadId,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentDownloadChunkRequest {
    pub attachment_id: AttachmentId,
    pub offset: u64,
    pub max_bytes: usize,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AttachmentDownloadChunk {
    pub attachment_id: AttachmentId,
    pub offset: u64,
    pub next_offset: u64,
    pub total_size: u64,
    pub done: bool,
    pub checksum_sha256: String,
    pub bytes_base64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MarkerId(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarkerCreateRequest {
    pub label: String,
    pub position: GeoPoint,
    pub topic_id: Option<TopicId>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarkerUpdatePositionRequest {
    pub marker_id: MarkerId,
    pub position: GeoPoint,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarkerRecord {
    pub marker_id: MarkerId,
    pub label: String,
    pub position: GeoPoint,
    pub topic_id: Option<TopicId>,
    pub updated_ts_ms: u64,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarkerListRequest {
    pub topic_id: Option<TopicId>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MarkerListResult {
    pub markers: Vec<MarkerRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct IdentityRef(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IdentityBundle {
    pub identity: IdentityRef,
    pub public_key: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IdentityImportRequest {
    pub bundle_base64: String,
    pub passphrase: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct IdentityResolveRequest {
    pub hash: String,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PaperMessageEnvelope {
    pub uri: String,
    pub transient_id: Option<String>,
    pub destination_hint: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteCommandRequest {
    pub command: String,
    pub target: Option<String>,
    pub payload: JsonValue,
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteCommandResponse {
    pub accepted: bool,
    pub payload: JsonValue,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct VoiceSessionId(pub String);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSessionState {
    New,
    Ringing,
    Active,
    Holding,
    Closed,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VoiceSessionOpenRequest {
    pub peer_id: String,
    pub codec_hint: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct VoiceSessionUpdateRequest {
    pub session_id: VoiceSessionId,
    pub state: VoiceSessionState,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[cfg(test)]
mod tests {
    use super::VoiceSessionState;

    #[test]
    fn voice_session_state_deserializes_unknown_variant() {
        let value = serde_json::json!("paused_by_gateway");
        let state: VoiceSessionState =
            serde_json::from_value(value).expect("unknown voice state should map to Unknown");
        assert_eq!(state, VoiceSessionState::Unknown);
    }
}
