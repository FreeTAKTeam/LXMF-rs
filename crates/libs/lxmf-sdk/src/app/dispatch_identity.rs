use super::dispatch::{envelope_result, invalid_envelope};
use super::envelope::EnvelopeResponse;
use super::errors::Error;
use super::node::Client;
use crate::domain::{
    ContactListRequest, ContactUpdateRequest, IdentityAnnounceRequest, IdentityBootstrapRequest,
    IdentityCreateRequest, PresenceListRequest, WorkflowAttachmentReportRequest,
    WorkflowMissionUpdateRequest, WorkflowPeerReadyRequest, WorkflowTopicSyncRequest,
};
use crate::SdkBackend;
use serde_json::Value as JsonValue;

impl<B: SdkBackend> Client<B> {
    pub(super) fn dispatch_identity_workflow_envelope(
        &self,
        canonical_id: super::operations::OperationId,
        correlation_id: Option<String>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, Error> {
        match canonical_id.as_str() {
            "app.identity.list" => Ok(envelope_result(
                canonical_id,
                correlation_id,
                serde_json::to_value(self.backend.identity_list().map_err(Error::from)?)
                    .expect("identity list should serialize"),
            )),
            "app.identity.create" => {
                let req: IdentityCreateRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid identity create payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let identity = self.backend.identity_create(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(identity).expect("created identity should serialize"),
                ))
            }
            "app.identity.announce" => {
                if payload.as_object().is_some_and(serde_json::Map::is_empty) {
                    self.backend.identity_announce_now().map_err(Error::from)?;
                    return Ok(envelope_result(
                        canonical_id,
                        correlation_id,
                        serde_json::json!({ "accepted": true }),
                    ));
                }
                let req: IdentityAnnounceRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid identity announce payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let announce = self.backend.identity_announce(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(announce).expect("identity announce should serialize"),
                ))
            }
            "app.identity.presence.list" => {
                let req: PresenceListRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid presence list payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.identity_presence_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("presence list should serialize"),
                ))
            }
            "app.contact.list" => {
                let req: ContactListRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid contact list payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.identity_contact_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("contact list should serialize"),
                ))
            }
            "app.contact.update" => {
                let req: ContactUpdateRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid contact update payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.identity_contact_update(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("contact update should serialize"),
                ))
            }
            "app.identity.bootstrap" => {
                let req: IdentityBootstrapRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid bootstrap payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.identity_bootstrap(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("bootstrap should serialize"),
                ))
            }
            "app.workflow.peer_ready" => {
                let req: WorkflowPeerReadyRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid workflow peer ready payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.workflow_peer_ready(req)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("workflow peer ready should serialize"),
                ))
            }
            "app.workflow.topic_sync" => {
                let req: WorkflowTopicSyncRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid workflow topic sync payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.workflow_topic_sync(req)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("workflow topic sync should serialize"),
                ))
            }
            "app.workflow.attachment_report_publish" => {
                let req: WorkflowAttachmentReportRequest = serde_json::from_value(payload)
                    .map_err(|err| {
                        invalid_envelope(
                            format!("invalid workflow attachment report payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.workflow_attachment_report_publish(req)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result)
                        .expect("workflow attachment report should serialize"),
                ))
            }
            "app.workflow.mission_update_send" => {
                let req: WorkflowMissionUpdateRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid workflow mission update payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.workflow_mission_update_send(req)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("workflow mission update should serialize"),
                ))
            }
            _ => unreachable!("identity/workflow dispatch called for unsupported operation"),
        }
    }
}
