use super::{OutboundDeliveryOptionsCompat, OUTBOUND_DELIVERY_OPTIONS_FIELD};
use reticulum::storage::messages::MessageRecord;
use serde_json::Value;

fn parse_u32_field(value: &Value) -> Option<u32> {
    match value {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn parse_bool_field(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn parse_string_field(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string())
}

#[cfg(reticulum_api_v2)]
pub(super) fn merge_outbound_delivery_options(
    api_options: &reticulum::rpc::OutboundDeliveryOptions,
    record: &MessageRecord,
) -> OutboundDeliveryOptionsCompat {
    let mut out = extract_outbound_delivery_options(record);
    if out.method.is_none() {
        out.method = api_options.method.clone();
    }
    if out.stamp_cost.is_none() {
        out.stamp_cost = api_options.stamp_cost;
    }
    out.include_ticket = api_options.include_ticket || out.include_ticket;
    out.try_propagation_on_fail =
        api_options.try_propagation_on_fail || out.try_propagation_on_fail;
    if out.ticket.is_none() {
        out.ticket = api_options.ticket.clone();
    }
    if out.source_private_key.is_none() {
        out.source_private_key = api_options.source_private_key.clone();
    }

    out
}

#[cfg(not(reticulum_api_v2))]
pub(super) fn merge_outbound_delivery_options(
    record: &MessageRecord,
) -> OutboundDeliveryOptionsCompat {
    extract_outbound_delivery_options(record)
}

fn extract_outbound_delivery_options(record: &MessageRecord) -> OutboundDeliveryOptionsCompat {
    let mut out = OutboundDeliveryOptionsCompat::default();
    let Some(fields) = record.fields.as_ref().and_then(Value::as_object) else {
        return out;
    };

    if let Some(options) = fields.get(OUTBOUND_DELIVERY_OPTIONS_FIELD).and_then(Value::as_object) {
        if let Some(method) = parse_string_field(options.get("method").unwrap_or(&Value::Null)) {
            out.method = Some(method);
        }
        if let Some(cost) = parse_u32_field(options.get("stamp_cost").unwrap_or(&Value::Null)) {
            out.stamp_cost = Some(cost);
        }
        if let Some(include_ticket) =
            parse_bool_field(options.get("include_ticket").unwrap_or(&Value::Null))
        {
            out.include_ticket = include_ticket;
        }
        if let Some(try_propagation_on_fail) =
            parse_bool_field(options.get("try_propagation_on_fail").unwrap_or(&Value::Null))
        {
            out.try_propagation_on_fail = try_propagation_on_fail;
        }
        if let Some(ticket) = parse_string_field(options.get("ticket").unwrap_or(&Value::Null)) {
            out.ticket = Some(ticket);
        }
        if let Some(source_private_key) =
            parse_string_field(options.get("source_private_key").unwrap_or(&Value::Null))
        {
            out.source_private_key = Some(source_private_key);
        }
    }

    if let Some(lxmf) = fields
        .get("_lxmf")
        .and_then(Value::as_object)
        .or_else(|| fields.get("lxmf").and_then(Value::as_object))
    {
        if out.method.is_none() {
            if let Some(method) = parse_string_field(lxmf.get("method").unwrap_or(&Value::Null)) {
                out.method = Some(method);
            }
        }
        if out.stamp_cost.is_none() {
            if let Some(cost) = parse_u32_field(lxmf.get("stamp_cost").unwrap_or(&Value::Null)) {
                out.stamp_cost = Some(cost);
            }
        }
        if let Some(include_ticket) =
            parse_bool_field(lxmf.get("include_ticket").unwrap_or(&Value::Null))
        {
            out.include_ticket = include_ticket;
        }
    }

    out
}
