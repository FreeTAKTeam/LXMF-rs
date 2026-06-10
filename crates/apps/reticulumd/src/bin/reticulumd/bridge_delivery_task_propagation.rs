use super::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;

impl DeliveryTask {
    pub(super) fn propagation_target_cost(&self, propagation_node_hex: &str) -> Option<u32> {
        let response = self
            .daemon
            .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
            .ok()?
            .result?;
        response
            .get("peers")
            .and_then(|value| value.as_array())
            .and_then(|rows| {
                rows.iter().find(|row| {
                    row.get("peer")
                        .and_then(|value| value.as_str())
                        .is_some_and(|peer| peer.eq_ignore_ascii_case(propagation_node_hex))
                })
            })
            .and_then(|row| row.get("propagation_stamp_cost"))
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
    }

    pub(super) fn record_propagation_payload_metadata(
        &self,
        propagation_payload: &propagation::PropagationPayload,
        target_cost: u32,
    ) {
        let _ = self.daemon.record_message_lxmf_metadata_entries(
            &self.message_id,
            [
                (
                    "propagation_transient_id".to_string(),
                    JsonValue::String(hex::encode(propagation_payload.transient_id)),
                ),
                ("propagation_packed".to_string(), JsonValue::Bool(true)),
                (
                    "propagation_packed_size".to_string(),
                    JsonValue::Number(serde_json::Number::from(propagation_payload.bytes.len())),
                ),
                (
                    "propagation_packed_base64".to_string(),
                    JsonValue::String(BASE64_STANDARD.encode(&propagation_payload.bytes)),
                ),
                (
                    "propagation_target_cost".to_string(),
                    JsonValue::Number(serde_json::Number::from(target_cost)),
                ),
                ("propagation_stamp_valid".to_string(), JsonValue::Bool(true)),
                (
                    "propagation_stamp_value".to_string(),
                    JsonValue::Number(serde_json::Number::from(propagation_payload.stamp_value)),
                ),
            ],
        );
    }

    pub(super) fn record_propagation_stamp_work_metadata(
        &self,
        state: &str,
        target_cost: u32,
        detail: Option<String>,
    ) {
        let mut entries = vec![
            ("propagation_stamp_state".to_string(), JsonValue::String(state.to_string())),
            (
                "propagation_target_cost".to_string(),
                JsonValue::Number(serde_json::Number::from(target_cost)),
            ),
        ];
        if let Some(detail) = detail {
            let key = if state == "ready" {
                "propagation_stamp_value"
            } else {
                "propagation_stamp_error"
            };
            let value = if key == "propagation_stamp_value" {
                detail
                    .parse::<u64>()
                    .ok()
                    .map(|value| JsonValue::Number(serde_json::Number::from(value)))
                    .unwrap_or(JsonValue::String(detail))
            } else {
                JsonValue::String(detail)
            };
            entries.push((key.to_string(), value));
        }
        let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
    }
}
