use super::dispatch::{envelope_result, invalid_envelope};
use super::envelope::EnvelopeResponse;
use super::errors::Error;
use super::node::Client;
use crate::domain::{
    MarkerCreateRequest, MarkerDeleteRequest, MarkerListRequest, MarkerUpdatePositionRequest,
    TelemetryQuery, VoiceSessionId, VoiceSessionOpenRequest, VoiceSessionUpdateRequest,
};
use crate::SdkBackend;
use serde_json::Value as JsonValue;

impl<B: SdkBackend> Client<B> {
    pub(super) fn dispatch_telemetry_marker_voice_envelope(
        &self,
        canonical_id: super::operations::OperationId,
        correlation_id: Option<String>,
        payload: JsonValue,
    ) -> Result<EnvelopeResponse, Error> {
        match canonical_id.as_str() {
            "app.telemetry.query" => {
                let req: TelemetryQuery = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid telemetry query payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.telemetry_query(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("telemetry query should serialize"),
                ))
            }
            "app.telemetry.subscribe" => {
                let req: TelemetryQuery = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid telemetry subscribe payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.telemetry_subscribe(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("telemetry subscribe should serialize"),
                ))
            }
            "app.marker.create" => {
                let req: MarkerCreateRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid marker create payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.marker_create(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("marker create should serialize"),
                ))
            }
            "app.marker.list" => {
                let req: MarkerListRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid marker list payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.marker_list(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("marker list should serialize"),
                ))
            }
            "app.marker.update_position" => {
                let req: MarkerUpdatePositionRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid marker update payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let result = self.backend.marker_update_position(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("marker update should serialize"),
                ))
            }
            "app.marker.delete" => {
                let req: MarkerDeleteRequest = serde_json::from_value(payload).map_err(|err| {
                    invalid_envelope(
                        format!("invalid marker delete payload: {err}"),
                        canonical_id.as_str(),
                    )
                })?;
                let result = self.backend.marker_delete(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(result).expect("marker delete should serialize"),
                ))
            }
            "app.voice.session.open" => {
                let req: VoiceSessionOpenRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid voice session open payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let session_id = self.backend.voice_session_open(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(session_id).expect("voice session id should serialize"),
                ))
            }
            "app.voice.session.update" => {
                let req: VoiceSessionUpdateRequest =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid voice session update payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                let state = self.backend.voice_session_update(req).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::to_value(state).expect("voice state should serialize"),
                ))
            }
            "app.voice.session.close" => {
                let session_id: VoiceSessionId =
                    serde_json::from_value(payload).map_err(|err| {
                        invalid_envelope(
                            format!("invalid voice session close payload: {err}"),
                            canonical_id.as_str(),
                        )
                    })?;
                self.backend.voice_session_close(session_id.clone()).map_err(Error::from)?;
                Ok(envelope_result(
                    canonical_id,
                    correlation_id,
                    serde_json::json!({
                        "accepted": true,
                        "session_id": session_id.0,
                    }),
                ))
            }
            _ => unreachable!("telemetry/marker/voice dispatch called for unsupported operation"),
        }
    }
}
