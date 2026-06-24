use super::delivery_task::{emit_receipt_event, PropagationPreparationContext};
use super::*;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;

impl DeliveryTask {
    pub(super) async fn propagation_preparation_context(
        &self,
    ) -> Option<PropagationPreparationContext> {
        let destination_identity = match self.resolve_destination_identity().await {
            Ok(Some(identity)) => identity,
            Ok(None) => return None,
            Err(err) => {
                log::warn!(
                    "[daemon] {} resolve destination identity failed: {err}",
                    self.message_id
                );
                return None;
            }
        };
        if self.abort_if_cancelled("propagation") {
            return None;
        }
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "recipient identity ready",
        );
        let Some(propagation_node_hex) = self.propagation_node_hex.clone() else {
            emit_receipt_event(
                &self.receipt_tx,
                ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: "failed: no outbound propagation node selected".to_string(),
                },
            );
            return None;
        };

        let propagation_hash = match parse_destination_hash_required(&propagation_node_hex) {
            Ok(hash) => AddressHash::new(hash),
            Err(err) => {
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent {
                        message_id: self.message_id.clone(),
                        status: format!("failed: {err}"),
                    },
                );
                return None;
            }
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "selected propagation node parsed",
        );
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "looking up propagation stamp cost",
        );
        let (target_cost, cost_source) = self
            .propagation_target_cost_reference_style(
                propagation_node_hex.as_str(),
                propagation_hash,
            )
            .await;
        let target_cost = target_cost.unwrap_or(propagation::DEFAULT_PROPAGATION_STAMP_COST);
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!("using propagation stamp cost={target_cost} source={cost_source}").as_str(),
        );
        Some(PropagationPreparationContext {
            destination_identity,
            propagation_node_hex,
            propagation_hash,
            target_cost,
        })
    }

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
        let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
            rmp_serde::from_slice(payload.bytes.as_slice()).map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid local propagation envelope: {err}"),
                )
            })?;
        let accepted_stamp_cost = self.daemon.propagation_min_accepted_stamp_cost();
        for message in messages.iter() {
            let transient_id = self
                .daemon
                .canonical_propagation_payload_bytes_at_cost(message, accepted_stamp_cost)?;
            self.daemon.ingest_client_propagation_payload_bytes_at_cost(
                message,
                Some(transient_id.as_str()),
                accepted_stamp_cost,
            )?;
        }
        log_delivery_trace(
            &self.message_id,
            propagation_node_hex,
            "propagation",
            format!("propagation stored locally messages={}", messages.len()).as_str(),
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

    pub(super) fn record_propagation_stamp_attempt_metadata(&self, target_cost: u32, attempt: u32) {
        let _ = self.daemon.record_message_lxmf_metadata_entries(
            &self.message_id,
            [
                (
                    "propagation_stamp_state".to_string(),
                    JsonValue::String("generating".to_string()),
                ),
                (
                    "propagation_target_cost".to_string(),
                    JsonValue::Number(serde_json::Number::from(target_cost)),
                ),
                (
                    "propagation_stamp_attempts".to_string(),
                    JsonValue::Number(serde_json::Number::from(attempt)),
                ),
                ("propagation_stamp_error".to_string(), JsonValue::Null),
                ("progress".to_string(), JsonValue::Number(0.into())),
            ],
        );
    }

    pub(super) fn record_propagation_stamp_retry_metadata(
        &self,
        target_cost: u32,
        attempt: u32,
        error: String,
    ) {
        let _ = self.daemon.record_message_lxmf_metadata_entries(
            &self.message_id,
            [
                ("propagation_stamp_state".to_string(), JsonValue::String("queued".to_string())),
                (
                    "propagation_target_cost".to_string(),
                    JsonValue::Number(serde_json::Number::from(target_cost)),
                ),
                (
                    "propagation_stamp_attempts".to_string(),
                    JsonValue::Number(serde_json::Number::from(attempt)),
                ),
                ("propagation_stamp_error".to_string(), JsonValue::String(error)),
                (
                    "propagation_stamp_next_retry_at".to_string(),
                    JsonValue::Number(serde_json::Number::from(now_secs_i64() + 1)),
                ),
                ("progress".to_string(), JsonValue::Number(0.into())),
            ],
        );
    }
}
