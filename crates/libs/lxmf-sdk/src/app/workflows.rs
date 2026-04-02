use super::discovery::{BootstrapRequest, ContactUpdate};
use super::errors::Error;
use super::node::Client;
use super::runtime::SendRequest;
use crate::domain::{
    AttachmentStoreRequest, TelemetryQuery, TopicCreateRequest, TopicPublishRequest,
    TopicSubscriptionRequest, WorkflowAttachmentReportRequest, WorkflowAttachmentReportResult,
    WorkflowMissionUpdateRequest, WorkflowMissionUpdateResult, WorkflowPeerReadyRequest,
    WorkflowPeerReadyResult, WorkflowTopicSyncRequest, WorkflowTopicSyncResult,
};
use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};
use crate::SdkBackend;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;

impl<B: SdkBackend> Client<B> {
    pub fn workflow_peer_ready(
        &self,
        request: WorkflowPeerReadyRequest,
    ) -> Result<WorkflowPeerReadyResult, Error> {
        let existing = self.find_contact(request.identity.0.as_str())?;
        let announce = request.announce.unwrap_or(true);
        let announced = if announce {
            self.announce_now()?;
            true
        } else {
            false
        };

        if let Some(contact) = existing {
            return Ok(WorkflowPeerReadyResult {
                identity: request.identity,
                contact: crate::domain::ContactRecord {
                    identity: crate::domain::IdentityRef(contact.identity.clone()),
                    display_name: contact.display_name,
                    trust_level: contact.trust_level,
                    bootstrap: contact.bootstrap,
                    updated_ts_ms: contact.updated_ts_ms,
                    metadata: contact.metadata,
                    extensions: contact.extensions,
                },
                was_created: false,
                announced,
            });
        }

        let bootstrap = request.bootstrap.unwrap_or(true);
        let contact = if bootstrap {
            self.bootstrap_identity(BootstrapRequest {
                identity: request.identity.0.clone(),
                auto_sync: true,
                extensions: request.extensions.clone(),
            })?
        } else {
            self.update_contact(ContactUpdate {
                identity: request.identity.0.clone(),
                display_name: request.display_name.clone(),
                trust_level: request.trust_level.clone(),
                bootstrap: Some(false),
                metadata: request.metadata.clone().into_iter().collect(),
                extensions: request.extensions.clone().into_iter().collect(),
            })?
        };

        Ok(WorkflowPeerReadyResult {
            identity: request.identity,
            contact: crate::domain::ContactRecord {
                identity: crate::domain::IdentityRef(contact.identity.clone()),
                display_name: contact.display_name,
                trust_level: contact.trust_level,
                bootstrap: contact.bootstrap,
                updated_ts_ms: contact.updated_ts_ms,
                metadata: contact.metadata,
                extensions: contact.extensions,
            },
            was_created: true,
            announced,
        })
    }

    pub fn workflow_topic_sync(
        &self,
        request: WorkflowTopicSyncRequest,
    ) -> Result<WorkflowTopicSyncResult, Error> {
        let existing = self.find_topic_by_path(request.topic_path.0.as_str(), 100)?;
        let (topic, was_created) = if let Some(topic) = existing {
            (topic, false)
        } else {
            let topic = self
                .backend
                .topic_create(TopicCreateRequest {
                    topic_path: Some(request.topic_path.clone()),
                    metadata: request.metadata.clone(),
                    extensions: request.extensions.clone(),
                })
                .map_err(Error::from)?;
            (topic, true)
        };
        let subscribed = self
            .backend
            .topic_subscribe(TopicSubscriptionRequest {
                topic_id: topic.topic_id.clone(),
                cursor: None,
                extensions: request.extensions.clone(),
            })
            .map_err(Error::from)?
            .accepted;
        let telemetry = self
            .backend
            .telemetry_query(TelemetryQuery {
                peer_id: None,
                topic_id: Some(topic.topic_id.clone()),
                from_ts_ms: None,
                to_ts_ms: None,
                limit: request.telemetry_limit,
                extensions: request.extensions,
            })
            .map_err(Error::from)?;
        Ok(WorkflowTopicSyncResult { topic, was_created, subscribed, telemetry })
    }

    pub fn workflow_attachment_report_publish(
        &self,
        request: WorkflowAttachmentReportRequest,
    ) -> Result<WorkflowAttachmentReportResult, Error> {
        let topic_sync = self.workflow_topic_sync(WorkflowTopicSyncRequest {
            topic_path: request.topic_path,
            metadata: request.topic_metadata,
            telemetry_limit: Some(0),
            extensions: request.extensions.clone(),
        })?;
        let attachment = self
            .backend
            .attachment_store(AttachmentStoreRequest {
                name: request.attachment.name,
                content_type: request.attachment.content_type,
                bytes_base64: request.attachment.bytes_base64,
                expires_ts_ms: None,
                topic_ids: vec![topic_sync.topic.topic_id.clone()],
                extensions: request.extensions.clone(),
            })
            .map_err(Error::from)?;
        let mut payload = JsonMap::new();
        if let Some(summary) = request.summary_payload {
            payload.insert("summary".to_owned(), summary);
        }
        payload.insert(
            "attachment_id".to_owned(),
            JsonValue::String(attachment.attachment_id.0.clone()),
        );
        payload.insert("attachment_name".to_owned(), JsonValue::String(attachment.name.clone()));
        payload
            .insert("content_type".to_owned(), JsonValue::String(attachment.content_type.clone()));
        let published = self
            .backend
            .topic_publish(TopicPublishRequest {
                topic_id: topic_sync.topic.topic_id.clone(),
                payload: JsonValue::Object(payload.into_iter().collect()),
                correlation_id: request.correlation_id,
                extensions: request.extensions,
            })
            .map_err(Error::from)?;
        Ok(WorkflowAttachmentReportResult { topic: topic_sync.topic, attachment, published })
    }

    pub fn workflow_mission_update_send(
        &self,
        request: WorkflowMissionUpdateRequest,
    ) -> Result<WorkflowMissionUpdateResult, Error> {
        const RESERVED_KEYS: &[&str] = &["content", "topic_id", "group_id", "file_attachments"];
        let conflicting = request
            .metadata
            .keys()
            .filter(|key| RESERVED_KEYS.contains(&key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !conflicting.is_empty() {
            return Err(Error::from(
                SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    SdkErrorCategory::Validation,
                    format!(
                        "mission metadata cannot override reserved fields: {}",
                        conflicting.join(", ")
                    ),
                )
                .with_user_actionable(true),
            ));
        }

        let peer = self.workflow_peer_ready(WorkflowPeerReadyRequest {
            identity: request.peer_identity.clone(),
            display_name: request.display_name,
            trust_level: request.trust_level,
            bootstrap: request.bootstrap,
            announce: request.announce,
            metadata: request.metadata.clone(),
            extensions: request.extensions.clone(),
        })?;

        let topic = if let Some(topic_path) = request.topic_path.clone() {
            Some(
                self.workflow_topic_sync(WorkflowTopicSyncRequest {
                    topic_path,
                    metadata: BTreeMap::new(),
                    telemetry_limit: Some(0),
                    extensions: request.extensions.clone(),
                })?
                .topic,
            )
        } else {
            None
        };

        let mut attachments = Vec::new();
        for attachment in request.attachments {
            let attachment = self
                .backend
                .attachment_store(AttachmentStoreRequest {
                    name: attachment.name,
                    content_type: attachment.content_type,
                    bytes_base64: attachment.bytes_base64,
                    expires_ts_ms: None,
                    topic_ids: topic
                        .as_ref()
                        .map(|topic| vec![topic.topic_id.clone()])
                        .unwrap_or_default(),
                    extensions: request.extensions.clone(),
                })
                .map_err(Error::from)?;
            attachments.push(attachment);
        }

        let mut payload: JsonMap<String, JsonValue> = request.metadata.into_iter().collect();
        payload.insert("content".to_owned(), JsonValue::String(request.content));
        if let Some(topic) = topic.as_ref() {
            payload.insert("topic_id".to_owned(), JsonValue::String(topic.topic_id.0.clone()));
            payload.insert("group_id".to_owned(), JsonValue::String(topic.topic_id.0.clone()));
        }
        if !attachments.is_empty() {
            payload.insert(
                "file_attachments".to_owned(),
                JsonValue::Array(
                    attachments
                        .iter()
                        .map(|attachment| {
                            JsonValue::Object(JsonMap::from_iter([
                                (
                                    "attachment_id".to_owned(),
                                    JsonValue::String(attachment.attachment_id.0.clone()),
                                ),
                                ("name".to_owned(), JsonValue::String(attachment.name.clone())),
                                (
                                    "content_type".to_owned(),
                                    JsonValue::String(attachment.content_type.clone()),
                                ),
                                (
                                    "byte_len".to_owned(),
                                    JsonValue::Number(serde_json::Number::from(
                                        attachment.byte_len,
                                    )),
                                ),
                            ]))
                        })
                        .collect(),
                ),
            );
        }
        let source =
            self.identities()?.into_iter().next().map(|identity| identity.identity).ok_or_else(
                || {
                    Error::from(
                        SdkError::new(
                            code::RUNTIME_INVALID_STATE,
                            SdkErrorCategory::Runtime,
                            "mission update requires an active identity",
                        )
                        .with_user_actionable(true),
                    )
                },
            )?;
        let mut send_request =
            SendRequest::new(source, request.peer_identity.0, JsonValue::Object(payload));
        if let Some(correlation_id) = request.correlation_id {
            send_request = send_request.with_correlation_id(correlation_id);
        }
        if let Some(idempotency_key) = request.idempotency_key {
            send_request = send_request.with_idempotency_key(idempotency_key);
        }
        let receipt = self.send(send_request)?;
        Ok(WorkflowMissionUpdateResult {
            peer,
            message_id: receipt.message_id.into(),
            topic,
            attachments,
        })
    }
}
