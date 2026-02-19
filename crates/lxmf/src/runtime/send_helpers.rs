use crate::wire_fields::contains_attachment_aliases;
use reticulum::delivery::{
    send_outcome_is_sent as shared_send_outcome_is_sent,
    send_outcome_status as shared_send_outcome_status,
    strip_destination_prefix as shared_strip_destination_prefix,
};
use reticulum::transport::SendPacketOutcome;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeliveryMethod {
    Auto,
    Direct,
    Opportunistic,
    Propagated,
}

pub(super) fn parse_delivery_method(method: Option<&str>) -> DeliveryMethod {
    let Some(method) = method.map(str::trim).filter(|value| !value.is_empty()) else {
        return DeliveryMethod::Auto;
    };

    match method.to_ascii_lowercase().as_str() {
        "direct" | "link" => DeliveryMethod::Direct,
        "opportunistic" => DeliveryMethod::Opportunistic,
        "propagated" | "propagation" | "relay" => DeliveryMethod::Propagated,
        _ => DeliveryMethod::Auto,
    }
}

pub(super) fn can_send_opportunistic(fields: Option<&Value>, payload_len: usize) -> bool {
    const MAX_OPPORTUNISTIC_BYTES: usize = 295;
    payload_len <= MAX_OPPORTUNISTIC_BYTES && !fields_contain_attachments(fields)
}

fn fields_contain_attachments(fields: Option<&Value>) -> bool {
    contains_attachment_aliases(fields)
}

pub(super) fn send_outcome_is_sent(outcome: SendPacketOutcome) -> bool {
    shared_send_outcome_is_sent(outcome)
}

pub(super) fn send_outcome_status(method: &str, outcome: SendPacketOutcome) -> String {
    shared_send_outcome_status(method, outcome)
}

pub(super) fn opportunistic_payload<'a>(payload: &'a [u8], destination: &[u8; 16]) -> &'a [u8] {
    shared_strip_destination_prefix(payload, destination)
}
