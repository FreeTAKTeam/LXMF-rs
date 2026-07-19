use lxmf::inbound_decode::InboundPayloadMode;
use rns_rpc::{MessageRecord, RpcDaemon, RpcEvent};
use serde_json::{json, Value};

pub(crate) struct InboundDropEvent<'a> {
    pub(crate) reason: &'a str,
    pub(crate) delivery_kind: InboundDeliveryKind,
    pub(crate) raw_destination_hex: &'a str,
    pub(crate) destination: [u8; 16],
    pub(crate) payload_mode: InboundPayloadMode,
    pub(crate) bytes_len: usize,
    pub(crate) detail: Option<String>,
    pub(crate) record: Option<&'a MessageRecord>,
}

#[derive(Clone, Copy)]
pub(crate) enum InboundDeliveryKind {
    Packet,
    Propagation,
    Resource,
}

fn inbound_drop_event_payload(event: InboundDropEvent<'_>) -> Value {
    let mut payload = json!({
        "reason": event.reason,
        "delivery_kind": inbound_delivery_kind_name(event.delivery_kind),
        "raw_destination_hash": event.raw_destination_hex,
        "resolved_destination_hash": hex::encode(event.destination),
        "payload_mode": inbound_payload_mode_name(event.payload_mode),
        "bytes_len": event.bytes_len,
    });
    if let Some(detail) = inbound_drop_detail(event.reason, event.detail) {
        payload["detail"] = Value::String(detail);
    }
    if let Some(record) = event.record {
        payload["dropped_message_id"] = Value::String(record.id.clone());
        payload["source_hash"] = Value::String(record.source.clone());
        payload["destination_hash"] = Value::String(record.destination.clone());
    }
    payload
}

pub(crate) fn emit_inbound_drop_event(daemon: &RpcDaemon, event: InboundDropEvent<'_>) {
    let payload = inbound_drop_event_payload(event);
    daemon.publish_event(RpcEvent { event_type: "inbound_dropped".to_string(), payload });
}

pub(crate) fn emit_propagation_predecode_drop_event(
    daemon: &RpcDaemon,
    destination: [u8; 16],
    transient_payload: &[u8],
    reason: &'static str,
    detail: impl Into<String>,
) {
    let raw_destination_hex =
        propagated_transient_raw_destination_hex(destination, transient_payload);
    emit_inbound_drop_event(
        daemon,
        InboundDropEvent {
            reason,
            delivery_kind: InboundDeliveryKind::Propagation,
            raw_destination_hex: raw_destination_hex.as_str(),
            destination,
            payload_mode: InboundPayloadMode::FullWire,
            bytes_len: transient_payload.len(),
            detail: Some(detail.into()),
            record: None,
        },
    );
}

pub(crate) fn emit_propagation_duplicate_drop_event(
    daemon: &RpcDaemon,
    destination: [u8; 16],
    transient_payload: &[u8],
    transient_id: &str,
    detail: &'static str,
) {
    let raw_destination_hex =
        propagated_transient_raw_destination_hex(destination, transient_payload);
    let mut payload = inbound_drop_event_payload(InboundDropEvent {
        reason: "duplicate",
        delivery_kind: InboundDeliveryKind::Propagation,
        raw_destination_hex: raw_destination_hex.as_str(),
        destination,
        payload_mode: InboundPayloadMode::FullWire,
        bytes_len: transient_payload.len(),
        detail: Some(detail.to_string()),
        record: None,
    });
    // Propagation transient IDs are protocol-visible content identifiers used by
    // queue and duplicate-accounting APIs; keep them structured, not in detail.
    payload["transient_id"] = Value::String(transient_id.to_string());
    daemon.publish_event(RpcEvent { event_type: "inbound_dropped".to_string(), payload });
}

fn propagated_transient_raw_destination_hex(
    destination: [u8; 16],
    transient_payload: &[u8],
) -> String {
    if transient_payload.len() >= 16 {
        hex::encode(&transient_payload[..16])
    } else {
        hex::encode(destination)
    }
}

fn inbound_payload_mode_name(mode: InboundPayloadMode) -> &'static str {
    match mode {
        InboundPayloadMode::FullWire => "full_wire",
        InboundPayloadMode::DestinationStripped => "destination_stripped",
    }
}

fn inbound_delivery_kind_name(kind: InboundDeliveryKind) -> &'static str {
    match kind {
        InboundDeliveryKind::Packet => "packet",
        InboundDeliveryKind::Propagation => "propagation",
        InboundDeliveryKind::Resource => "resource",
    }
}

fn inbound_drop_detail(reason: &str, detail: Option<String>) -> Option<String> {
    let detail = detail.filter(|value| !value.is_empty())?;
    if reason == "stamp_policy_rejected" {
        return Some("invalid LXMF stamp".to_string());
    }
    Some(detail)
}
