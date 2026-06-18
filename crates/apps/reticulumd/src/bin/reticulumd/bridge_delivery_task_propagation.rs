use super::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;

impl DeliveryTask {
    pub(super) async fn propagation_target_cost_reference_style(
        &self,
        propagation_node_hex: &str,
        propagation_hash: AddressHash,
    ) -> (Option<u32>, &'static str) {
        let (_peer, cost, source) =
            self.daemon.outbound_propagation_cost_lookup(Some(propagation_node_hex));
        if cost.is_some() {
            return (cost, source);
        }

        self.transport.request_path(&propagation_hash, None, None).await;
        log_delivery_trace(
            &self.message_id,
            propagation_node_hex,
            "propagation_target_cost",
            "path-requested",
        );
        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        while tokio::time::Instant::now() < deadline {
            let (_peer, cost, _source) =
                self.daemon.outbound_propagation_cost_lookup(Some(propagation_node_hex));
            if cost.is_some() {
                return (cost, "path_request");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        (None, "unavailable")
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

    pub(super) fn selected_propagation_node_is_local(&self, propagation_node_hex: &str) -> bool {
        self.daemon
            .local_propagation_hash()
            .is_some_and(|local_hash| local_hash.eq_ignore_ascii_case(propagation_node_hex))
    }

    pub(super) fn store_local_propagation_payload(
        &self,
        propagation_node_hex: &str,
        payload: &propagation::PropagationPayload,
    ) -> Result<(), std::io::Error> {
        log_delivery_trace(
            &self.message_id,
            propagation_node_hex,
            "propagation",
            "local propagation node selected",
        );
        let response = self.daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "propagation_ingest".to_string(),
            params: Some(json!({
                "payload_hex": hex::encode(payload.bytes.as_slice()),
            })),
        })?;
        if let Some(error) = response.error {
            return Err(std::io::Error::other(error.message));
        }
        log_delivery_trace(
            &self.message_id,
            propagation_node_hex,
            "propagation",
            "propagation stored locally",
        );
        Ok(())
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
        if state != "failed" {
            entries.push(("propagation_stamp_error".to_string(), JsonValue::Null));
        }
        let _ = self.daemon.record_message_lxmf_metadata_entries(&self.message_id, entries);
    }
}
