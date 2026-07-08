use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct StreamGapDetails {
    pub expected_seq_no: Option<u64>,
    pub observed_seq_no: Option<u64>,
    pub dropped_count: u64,
    pub recovery_required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct InboundMessageDetails {
    pub message_id: Option<String>,
    pub source_hash: Option<String>,
    pub destination_hash: Option<String>,
    pub delivery_kind: Option<String>,
    pub lxmf_bytes_hex: Option<String>,
    pub receipt_status: Option<String>,
    pub signature_checked: Option<bool>,
    pub signature_valid: Option<bool>,
    pub signature_status: Option<String>,
    pub stamp_checked: Option<bool>,
    pub stamp_valid: Option<bool>,
    pub stamp_status: Option<String>,
    pub propagation_stamp_checked: Option<bool>,
    pub propagation_stamp_valid: Option<bool>,
    pub method: Option<u64>,
    pub transport_encrypted: Option<bool>,
    pub transport_encryption: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct InboundDropDetails {
    pub reason: Option<String>,
    pub delivery_kind: Option<String>,
    pub raw_destination_hash: Option<String>,
    pub resolved_destination_hash: Option<String>,
    pub source_hash: Option<String>,
    pub destination_hash: Option<String>,
    pub dropped_message_id: Option<String>,
    pub payload_mode: Option<String>,
    pub bytes_len: Option<u64>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeliveryLifecycleDetails {
    pub state: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub receipt_status: Option<String>,
    pub delivery_kind: Option<String>,
    pub packet_hash: Option<String>,
    pub resource_hash: Option<String>,
    pub peer: Option<String>,
    pub method: Option<String>,
    pub bytes: Option<u64>,
    pub link_id: Option<String>,
    pub reason: Option<String>,
}

fn json_str(value: &JsonValue, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(ToOwned::to_owned)
}

fn nested_json<'a>(value: &'a JsonValue, path: &[&str]) -> Option<&'a JsonValue> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn nested_json_str(value: &JsonValue, path: &[&str]) -> Option<String> {
    nested_json(value, path)?.as_str().map(ToOwned::to_owned)
}

fn nested_json_bool(value: &JsonValue, path: &[&str]) -> Option<bool> {
    nested_json(value, path)?.as_bool()
}

fn nested_json_u64(value: &JsonValue, path: &[&str]) -> Option<u64> {
    nested_json(value, path)?.as_u64()
}

pub(super) fn inbound_message_details(payload: &JsonValue) -> InboundMessageDetails {
    let message = payload.get("message").unwrap_or(payload);
    InboundMessageDetails {
        message_id: json_str(message, "id").or_else(|| json_str(payload, "message_id")),
        source_hash: json_str(message, "source").or_else(|| json_str(payload, "source_hash")),
        destination_hash: json_str(message, "destination")
            .or_else(|| json_str(payload, "destination_hash")),
        delivery_kind: json_str(payload, "delivery_kind"),
        lxmf_bytes_hex: json_str(payload, "lxmf_bytes_hex"),
        receipt_status: json_str(message, "receipt_status")
            .or_else(|| json_str(payload, "receipt_status")),
        signature_checked: nested_json_bool(message, &["fields", "_lxmf", "signature_checked"]),
        signature_valid: nested_json_bool(message, &["fields", "_lxmf", "signature_valid"]),
        signature_status: nested_json_str(message, &["fields", "_lxmf", "signature_status"]),
        stamp_checked: nested_json_bool(message, &["fields", "_lxmf", "stamp_checked"]),
        stamp_valid: nested_json_bool(message, &["fields", "_lxmf", "stamp_valid"]),
        stamp_status: nested_json_str(message, &["fields", "_lxmf", "stamp_status"]),
        propagation_stamp_checked: nested_json_bool(
            message,
            &["fields", "_lxmf", "propagation_stamp_checked"],
        ),
        propagation_stamp_valid: nested_json_bool(
            message,
            &["fields", "_lxmf", "propagation_stamp_valid"],
        ),
        method: nested_json_u64(message, &["fields", "_lxmf", "method"]),
        transport_encrypted: nested_json_bool(message, &["fields", "_lxmf", "transport_encrypted"]),
        transport_encryption: nested_json_str(
            message,
            &["fields", "_lxmf", "transport_encryption"],
        ),
    }
}

pub(super) fn inbound_drop_details(payload: &JsonValue) -> InboundDropDetails {
    InboundDropDetails {
        reason: json_str(payload, "reason"),
        delivery_kind: json_str(payload, "delivery_kind"),
        raw_destination_hash: json_str(payload, "raw_destination_hash"),
        resolved_destination_hash: json_str(payload, "resolved_destination_hash"),
        source_hash: json_str(payload, "source_hash"),
        destination_hash: json_str(payload, "destination_hash"),
        dropped_message_id: json_str(payload, "dropped_message_id"),
        payload_mode: json_str(payload, "payload_mode"),
        bytes_len: payload.get("bytes_len").and_then(JsonValue::as_u64),
        detail: json_str(payload, "detail"),
    }
}

pub(super) fn delivery_lifecycle_details(payload: &JsonValue) -> DeliveryLifecycleDetails {
    let message = payload.get("message").unwrap_or(payload);
    DeliveryLifecycleDetails {
        state: json_str(payload, "state")
            .or_else(|| normalized_receipt_state(payload).ok().flatten()),
        from: json_str(payload, "from"),
        to: json_str(payload, "to"),
        receipt_status: json_str(message, "receipt_status")
            .or_else(|| json_str(payload, "receipt_status"))
            .or_else(|| json_str(payload, "status")),
        delivery_kind: json_str(payload, "delivery_kind"),
        packet_hash: json_str(payload, "packet_hash"),
        resource_hash: json_str(payload, "resource_hash"),
        peer: json_str(payload, "peer").or_else(|| json_str(payload, "peer_id")),
        method: json_str(payload, "method"),
        bytes: payload.get("bytes").and_then(JsonValue::as_u64),
        link_id: json_str(payload, "link_id"),
        reason: json_str(payload, "reason").or_else(|| json_str(payload, "detail")),
    }
}

pub(super) fn normalized_receipt_state(
    payload: &JsonValue,
) -> Result<Option<String>, &'static str> {
    let status_val = payload
        .get("message")
        .and_then(|message| message.get("receipt_status").or_else(|| message.get("status")))
        .or_else(|| payload.get("receipt_status"))
        .or_else(|| payload.get("status"))
        .or_else(|| payload.get("state"))
        .or_else(|| payload.get("receipt").and_then(|receipt| receipt.get("status")));
    let Some(status_val) = status_val else { return Ok(None) };
    let status = status_val.as_str().ok_or("receipt status is not a string")?;
    Ok(Some(status.split(':').next().unwrap_or(status).trim().to_ascii_lowercase()))
}
