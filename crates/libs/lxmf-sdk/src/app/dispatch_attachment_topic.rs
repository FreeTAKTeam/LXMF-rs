use super::dispatch::{envelope_result, invalid_envelope};
use super::envelope::EnvelopeResponse;
use super::errors::Error;
use super::node::Client;
use crate::domain::{
    AttachmentDownloadChunkRequest, AttachmentId, AttachmentListRequest, AttachmentStoreRequest,
    AttachmentUploadChunkRequest, AttachmentUploadCommitRequest, AttachmentUploadStartRequest,
    TopicCreateRequest, TopicId, TopicListRequest, TopicPublishRequest, TopicSubscriptionRequest,
};
use crate::SdkBackend;
use serde::Deserialize;
use serde_json::Value as JsonValue;

#[derive(Clone, Debug, Deserialize)]
struct AttachmentAssociateTopicPayload {
    attachment_id: AttachmentId,
    topic_id: TopicId,
}

impl<B: SdkBackend> Client<B> {
    pub(super) fn dispatch_attachment_topic_envelope(
        &self,
        canonical_id: super::operations::OperationId,
        correlation_id: Option<String>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, Error> {
        match canonical_id.as_str() {
            "app.attachment.store" => {
                let req: AttachmentStoreRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment store payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_store(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("attachment store should serialize"),
                ))
            }
            "app.attachment.get" => {
                let attachment_id: AttachmentId =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment get payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_get(attachment_id).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("attachment get should serialize"),
                ))
            }
            "app.attachment.list" => {
                let req: AttachmentListRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment list payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("attachment list should serialize"),
                ))
            }
            "app.attachment.delete" => {
                let attachment_id: AttachmentId =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment delete payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_delete(attachment_id).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("attachment delete should serialize"),
                ))
            }
            "app.attachment.associate_topic" => {
                let req: AttachmentAssociateTopicPayload = serde_json::from_value(payload)
                    .map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment associate payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self
                    .backend
                    .attachment_associate_topic(req.attachment_id, req.topic_id)
                    .map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("attachment associate should serialize"),
                ))
            }
            "app.attachment.upload_start" => {
                let req: AttachmentUploadStartRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment upload start payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_upload_start(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result)
                        .expect("attachment upload session should serialize"),
                ))
            }
            "app.attachment.upload_chunk" => {
                let req: AttachmentUploadChunkRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment upload chunk payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_upload_chunk(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result)
                        .expect("attachment upload chunk ack should serialize"),
                ))
            }
            "app.attachment.upload_commit" => {
                let req: AttachmentUploadCommitRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment upload commit payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_upload_commit(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result)
                        .expect("attachment upload commit should serialize"),
                ))
            }
            "app.attachment.download_chunk" => {
                let req: AttachmentDownloadChunkRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid attachment download chunk payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.attachment_download_chunk(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result)
                        .expect("attachment download chunk should serialize"),
                ))
            }
            "app.topic.create" => {
                let req: TopicCreateRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid topic create payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.topic_create(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("topic create should serialize"),
                ))
            }
            "app.topic.get" => {
                let topic_id: TopicId = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid topic get payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.topic_get(topic_id).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("topic get should serialize"),
                ))
            }
            "app.topic.list" => {
                let req: TopicListRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid topic list payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.topic_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("topic list should serialize"),
                ))
            }
            "app.topic.subscribe" => {
                let req: TopicSubscriptionRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid topic subscribe payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.topic_subscribe(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("topic subscribe should serialize"),
                ))
            }
            "app.topic.unsubscribe" => {
                let topic_id: TopicId = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid topic unsubscribe payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.topic_unsubscribe(topic_id).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("topic unsubscribe should serialize"),
                ))
            }
            "app.topic.publish" => {
                let req: TopicPublishRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid topic publish payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.topic_publish(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("topic publish should serialize"),
                ))
            }
            _ => unreachable!("attachment/topic dispatch called for unsupported operation"),
        }
    }
}
