use super::{
    clean_non_empty, generate_message_id, PreparedSendMessage, ProfileSettings, SendMessageRequest,
    INFERRED_TRANSPORT_BIND,
};
use crate::LxmfError;
use serde_json::{json, Value};

pub(super) fn annotate_response_meta(result: &mut Value, profile: &str, rpc_endpoint: &str) {
    let Some(root) = result.as_object_mut() else {
        return;
    };
    if !root.get("meta").map(Value::is_object).unwrap_or(false) {
        root.insert("meta".to_string(), serde_json::json!({}));
    }
    let Some(meta) = root.get_mut("meta").and_then(Value::as_object_mut) else {
        return;
    };

    if meta.get("contract_version").map(Value::is_null).unwrap_or(true) {
        meta.insert("contract_version".to_string(), Value::String("v2".to_string()));
    }
    if meta.get("profile").map(Value::is_null).unwrap_or(true) {
        meta.insert("profile".to_string(), Value::String(profile.to_string()));
    }
    if meta.get("rpc_endpoint").map(Value::is_null).unwrap_or(true) {
        meta.insert("rpc_endpoint".to_string(), Value::String(rpc_endpoint.to_string()));
    }
}

pub(super) fn build_send_params_with_source(
    request: SendMessageRequest,
    source: String,
) -> Result<PreparedSendMessage, LxmfError> {
    let destination = clean_non_empty(Some(request.destination))
        .ok_or_else(|| LxmfError::Io("destination is required".to_string()))?;
    let id = clean_non_empty(request.id).unwrap_or_else(generate_message_id);

    let mut params = json!({
        "id": id,
        "source": source,
        "destination": destination,
        "title": request.title,
        "content": request.content,
    });

    if let Some(fields) = request.fields {
        params["fields"] = fields;
    }
    if let Some(method) = clean_non_empty(request.method) {
        params["method"] = Value::String(method);
    }
    if let Some(stamp_cost) = request.stamp_cost {
        params["stamp_cost"] = Value::from(stamp_cost);
    }
    if request.include_ticket {
        params["include_ticket"] = Value::Bool(true);
    }
    if request.try_propagation_on_fail {
        params["try_propagation_on_fail"] = Value::Bool(true);
    }
    if let Some(source_private_key) = clean_non_empty(request.source_private_key) {
        params["source_private_key"] = Value::String(source_private_key);
    }

    Ok(PreparedSendMessage { id, source, destination, params })
}

pub(super) fn resolve_transport(
    settings: &ProfileSettings,
    has_enabled_interfaces: bool,
) -> (Option<String>, bool) {
    if let Some(value) = clean_non_empty(settings.transport.clone()) {
        return (Some(value), false);
    }
    if has_enabled_interfaces {
        return (Some(INFERRED_TRANSPORT_BIND.to_string()), true);
    }
    (None, false)
}
