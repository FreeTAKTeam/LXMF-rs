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
