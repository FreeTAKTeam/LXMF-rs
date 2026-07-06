use crate::backend::lxmf_wire_fields_from_payload;
use crate::types::{BatchSendItem, BatchSendRequest, SendRequest};
use serde_json::{json, Value as JsonValue};

pub(super) fn send_params(req: SendRequest, message_id: String) -> JsonValue {
    let SendRequest {
        source,
        destination,
        payload,
        delivery_method,
        stamp_cost,
        include_ticket,
        try_propagation_on_fail,
        idempotency_key,
        ttl_ms,
        correlation_id,
        extensions,
    } = req;
    let content = message_content(&payload);
    let title =
        payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();
    let fields = lxmf_wire_fields_from_payload(payload);
    json!({
        "id": message_id,
        "source": source,
        "destination": destination,
        "title": title,
        "content": content,
        "fields": fields,
        "method": delivery_method,
        "stamp_cost": stamp_cost,
        "include_ticket": include_ticket,
        "try_propagation_on_fail": try_propagation_on_fail,
        "idempotency_key": idempotency_key,
        "ttl_ms": ttl_ms,
        "correlation_id": correlation_id,
        "extensions": extensions,
    })
}

pub(super) fn batch_params(req: BatchSendRequest) -> JsonValue {
    let messages = req.messages.into_iter().map(batch_item_params).collect::<Vec<_>>();
    json!({
        "batch_id": req.batch_id,
        "source": req.source,
        "messages": messages,
    })
}

fn batch_item_params(item: BatchSendItem) -> JsonValue {
    let BatchSendItem {
        id,
        destination,
        payload,
        delivery_method,
        stamp_cost,
        include_ticket,
        try_propagation_on_fail,
        idempotency_key,
        ttl_ms,
        correlation_id,
        extensions,
    } = item;
    let content = message_content(&payload);
    let title =
        payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();
    let fields = lxmf_wire_fields_from_payload(payload);
    json!({
        "id": id,
        "destination": destination,
        "title": title,
        "content": content,
        "fields": fields,
        "method": delivery_method,
        "stamp_cost": stamp_cost,
        "include_ticket": include_ticket,
        "try_propagation_on_fail": try_propagation_on_fail,
        "idempotency_key": idempotency_key,
        "ttl_ms": ttl_ms,
        "correlation_id": correlation_id,
        "extensions": extensions,
    })
}

fn message_content(payload: &JsonValue) -> String {
    payload
        .get("content")
        .or_else(|| payload.get("body"))
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| payload.to_string())
}
