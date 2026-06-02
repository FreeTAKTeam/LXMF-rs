use super::envelope::EnvelopeResponse;
use super::errors::Error;
use super::node::Client;
use crate::SdkBackend;
use serde_json::Value as JsonValue;

impl<B: SdkBackend> Client<B> {
    pub(super) fn dispatch_content_envelope(
        &self,
        canonical_id: super::operations::OperationId,
        correlation_id: Option<String>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, Error> {
        match canonical_id.as_str() {
            "app.attachment.store"
            | "app.attachment.get"
            | "app.attachment.list"
            | "app.attachment.delete"
            | "app.attachment.associate_topic"
            | "app.attachment.upload_start"
            | "app.attachment.upload_chunk"
            | "app.attachment.upload_commit"
            | "app.attachment.download_chunk"
            | "app.topic.create"
            | "app.topic.get"
            | "app.topic.list"
            | "app.topic.subscribe"
            | "app.topic.unsubscribe"
            | "app.topic.publish" => {
                self.dispatch_attachment_topic_envelope(canonical_id, correlation_id, payload)
            }
            "app.telemetry.query"
            | "app.telemetry.subscribe"
            | "app.marker.create"
            | "app.marker.list"
            | "app.marker.update_position"
            | "app.marker.delete"
            | "app.voice.session.open"
            | "app.voice.session.update"
            | "app.voice.session.close" => {
                self.dispatch_telemetry_marker_voice_envelope(canonical_id, correlation_id, payload)
            }
            _ => unreachable!("content dispatch called for unsupported operation"),
        }
    }
}
