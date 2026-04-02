use super::envelope::{Envelope, EnvelopeKind, EnvelopeResponse};
use super::errors::Error;
use super::node::Client;
use super::runtime::{Config, SendRequest};
use crate::domain::RemoteCommandRequest;
use crate::{EventCursor, SdkBackend, ShutdownMode};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

impl<B: SdkBackend> Client<B> {
    pub fn execute_envelope(&self, envelope: Envelope) -> Result<EnvelopeResponse, Error> {
        let registry = self.operation_registry()?;
        let envelope = envelope.normalized(&registry).map_err(|err| match err {
            super::envelope::EnvelopeValidationError::UnknownOperation { operation_id } => {
                invalid_envelope("unknown operation id", operation_id.as_str())
            }
            super::envelope::EnvelopeValidationError::KindMismatch { operation_id, .. } => {
                invalid_envelope(
                    "envelope kind does not match registered operation kind",
                    operation_id.as_str(),
                )
            }
        })?;
        let entry = registry
            .get(envelope.operation_id.as_str())
            .cloned()
            .expect("normalized envelope should resolve to a registered operation");
        let canonical_id = entry.id.clone();

        let Envelope {
            operation_id: _,
            kind: _,
            target,
            correlation_id,
            timeout_ms,
            payload,
            extensions,
        } = envelope;

        match canonical_id.as_str() {
            "app.runtime.start" => {
                let config: Config = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid runtime start payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let handle = self.start(config)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(handle).expect("runtime handle should serialize"),
                ))
            }
            "app.runtime.restart" => {
                let config: Config = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid runtime restart payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let handle = self.restart(config)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(handle).expect("runtime handle should serialize"),
                ))
            }
            "app.runtime.stop" => {
                let mode = payload
                    .get("mode")
                    .cloned()
                    .map(serde_json::from_value::<ShutdownMode>)
                    .transpose()
                    .map_err(|err| {
                        invalid_envelope(
                            format!("invalid runtime stop payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?
                    .unwrap_or(ShutdownMode::Graceful);
                self.stop(mode.clone())?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::json!({
                        "accepted": true,
                        "mode": mode,
                    }),
                ))
            }
            "app.runtime.status" => Ok(envelope_result(
                canonical_id,
                correlation_id,
                serde_json::to_value(self.status()?).expect("runtime status should serialize"),
            )),
            "app.delivery.send" => {
                let request: SendRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid delivery send payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let receipt = self.send(request)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(receipt).expect("send receipt should serialize"),
                ))
            }
            "app.delivery.status" => {
                let message_id = payload
                    .get("message_id")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        invalid_envelope(
                            "delivery status envelope requires payload.message_id",
                            canonical_id.as_str(),
                        )
                    })?;
                let status = self.delivery_status(message_id.to_owned())?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(status).expect("delivery status should serialize"),
                ))
            }
            "app.event.poll" => {
                let cursor = payload
                    .get("cursor")
                    .and_then(|value| value.as_str())
                    .map(|value| EventCursor(value.to_owned()));
                let max = payload
                    .get("max")
                    .and_then(|value| value.as_u64())
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(32)
                    .max(1);
                let batch = self.backend.poll_events(cursor, max).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(batch).expect("event batch should serialize"),
                ))
            }
            "app.identity.list"
            | "app.identity.announce"
            | "app.identity.presence.list"
            | "app.contact.list"
            | "app.contact.update"
            | "app.identity.bootstrap"
            | "app.workflow.peer_ready"
            | "app.workflow.topic_sync"
            | "app.workflow.attachment_report_publish"
            | "app.workflow.mission_update_send" => {
                self.dispatch_identity_workflow_envelope(canonical_id, correlation_id, payload)
            }
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
            | "app.topic.publish"
            | "app.telemetry.query"
            | "app.telemetry.subscribe"
            | "app.marker.create"
            | "app.marker.list"
            | "app.marker.update_position"
            | "app.marker.delete"
            | "app.voice.session.open"
            | "app.voice.session.update"
            | "app.voice.session.close" => {
                self.dispatch_content_envelope(canonical_id, correlation_id, payload)
            }
            _ if matches!(entry.kind, super::operations::OperationKind::Query) => self
                .backend
                .envelope_execute(Envelope {
                    operation_id: canonical_id,
                    kind: EnvelopeKind::Query,
                    target,
                    correlation_id,
                    timeout_ms,
                    payload,
                    extensions,
                })
                .map_err(Error::from),
            _ => {
                let response = self
                    .backend
                    .command_invoke(RemoteCommandRequest {
                        command: canonical_id.as_str().to_owned(),
                        target,
                        payload,
                        timeout_ms,
                        extensions,
                    })
                    .map_err(Error::from)?;
                Ok(EnvelopeResponse {
                    operation_id: canonical_id,
                    kind: EnvelopeKind::Result,
                    accepted: response.accepted,
                    correlation_id,
                    payload: response.payload,
                    extensions: response.extensions,
                })
            }
        }
    }
}

pub(super) fn invalid_envelope(
    message: impl Into<String>,
    operation_id: impl Into<String>,
) -> Error {
    let mut details = BTreeMap::new();
    details.insert("operation_id".to_owned(), JsonValue::String(operation_id.into()));
    Error {
        code: super::errors::ErrorCode::ValidationInvalidArgument,
        category: super::errors::ErrorCategory::Validation,
        retryable: false,
        terminal: true,
        user_action_required: true,
        message: message.into(),
        details,
        cause_code: None,
    }
}

pub(super) fn envelope_result(
    operation_id: super::operations::OperationId,
    correlation_id: Option<String>,
    payload: JsonValue,
) -> EnvelopeResponse {
    EnvelopeResponse {
        operation_id,
        kind: EnvelopeKind::Result,
        accepted: true,
        correlation_id,
        payload,
        extensions: BTreeMap::new(),
    }
}
