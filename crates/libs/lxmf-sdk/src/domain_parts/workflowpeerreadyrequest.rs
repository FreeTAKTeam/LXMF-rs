#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowPeerReadyRequest {
    pub identity: IdentityRef,
    pub display_name: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub bootstrap: Option<bool>,
    pub announce: Option<bool>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowPeerReadyResult {
    pub identity: IdentityRef,
    pub contact: ContactRecord,
    pub was_created: bool,
    pub announced: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowTopicSyncRequest {
    pub topic_path: TopicPath,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    pub telemetry_limit: Option<usize>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowTopicSyncResult {
    pub topic: TopicRecord,
    pub was_created: bool,
    pub subscribed: bool,
    pub telemetry: Vec<TelemetryPoint>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowAttachmentDraft {
    pub name: String,
    pub content_type: String,
    pub bytes_base64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowAttachmentReportRequest {
    pub topic_path: TopicPath,
    pub attachment: WorkflowAttachmentDraft,
    pub summary_payload: Option<JsonValue>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub topic_metadata: BTreeMap<String, JsonValue>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowAttachmentReportResult {
    pub topic: TopicRecord,
    pub attachment: AttachmentMeta,
    pub published: Ack,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowMissionUpdateRequest {
    pub peer_identity: IdentityRef,
    pub content: String,
    pub topic_path: Option<TopicPath>,
    #[serde(default)]
    pub attachments: Vec<WorkflowAttachmentDraft>,
    #[serde(default)]
    pub metadata: BTreeMap<String, JsonValue>,
    pub correlation_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub display_name: Option<String>,
    pub trust_level: Option<TrustLevel>,
    pub bootstrap: Option<bool>,
    pub announce: Option<bool>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkflowMissionUpdateResult {
    pub peer: WorkflowPeerReadyResult,
    pub message_id: MessageId,
    pub topic: Option<TopicRecord>,
    pub attachments: Vec<AttachmentMeta>,
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
