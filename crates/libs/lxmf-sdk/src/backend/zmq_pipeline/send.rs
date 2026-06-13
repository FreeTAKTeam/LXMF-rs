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
    let content = payload
        .get("content")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| payload.to_string());
    let title =
        payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();
    let mut fields = match payload {
        JsonValue::Object(map) => JsonValue::Object(map),
        other => json!({ "payload": other }),
    };
    if let JsonValue::Object(map) = &mut fields {
        let mut sdk_meta = serde_json::Map::new();
        if let Some(value) = idempotency_key {
            sdk_meta.insert("idempotency_key".to_string(), JsonValue::String(value));
        }
        if let Some(value) = ttl_ms {
            sdk_meta.insert("ttl_ms".to_string(), JsonValue::from(value));
        }
        if let Some(value) = correlation_id {
            sdk_meta.insert("correlation_id".to_string(), JsonValue::String(value));
        }
        if !extensions.is_empty() {
            sdk_meta.insert(
                "extensions".to_string(),
                JsonValue::Object(extensions.into_iter().collect()),
            );
        }
        if !sdk_meta.is_empty() {
            map.insert("_sdk".to_string(), JsonValue::Object(sdk_meta));
        }
    }
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
    } = item;
    let content = payload
        .get("content")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| payload.to_string());
    let title =
        payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();
    let fields = match payload {
        JsonValue::Object(map) => JsonValue::Object(map),
        other => json!({ "payload": other }),
    };
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
    })
}
