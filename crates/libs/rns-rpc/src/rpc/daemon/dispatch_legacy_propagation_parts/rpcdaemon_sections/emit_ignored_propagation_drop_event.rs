impl RpcDaemon {
    fn emit_ignored_propagation_drop_event(
        &self,
        payload: &[u8],
        transient_id: Option<&str>,
        operation: &str,
        peer: Option<&str>,
    ) {
        let raw_destination_hex = payload.get(..16).map(hex::encode).unwrap_or_default();
        let resolved_destination_hex = raw_destination_hex.clone();
        let mut event_payload = json!({
            "reason": "delivery_policy_rejected",
            "delivery_kind": "propagation",
            "raw_destination_hash": raw_destination_hex,
            "resolved_destination_hash": resolved_destination_hex,
            "payload_mode": "full_wire",
            "bytes_len": payload.len(),
            "detail": "ignored propagation destination",
            "operation": operation,
        });
        if let Some(transient_id) = transient_id.filter(|value| !value.trim().is_empty()) {
            event_payload["transient_id"] = JsonValue::String(transient_id.to_string());
        }
        if let Some(peer) = peer.filter(|value| !value.trim().is_empty()) {
            event_payload["peer"] = JsonValue::String(peer.to_string());
        }
        self.publish_event(RpcEvent {
            event_type: "inbound_dropped".to_string(),
            payload: event_payload,
        });
    }
}
