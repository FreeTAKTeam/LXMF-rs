use crate::{ApiError, ApiResult, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand_core::{OsRng, RngCore};
use serde_json::{json, Value};
use std::io::Cursor;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE_MESSAGE_BYTES: usize = 900_000;

pub(super) async fn send_message(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    let message =
        body.get("lxmf_message").ok_or_else(|| ApiError::bad("lxmf_message is required"))?;
    let destination = message
        .get("destination_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("destination_hash is required"))?;
    if destination.len() != 32 || hex::decode(destination).is_err() {
        return Err(ApiError::bad("destination_hash must be 16 bytes of hexadecimal"));
    }
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("content is required"))?;
    let fields = message.get("fields").cloned().unwrap_or_else(|| json!({}));
    let fields = fields.as_object().ok_or_else(|| ApiError::bad("fields must be an object"))?;
    let wire_fields = attachment_fields_for_wire(fields, content.len())?;
    let method = body
        .get("delivery_method")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_str()
                .filter(|value| matches!(*value, "direct" | "opportunistic" | "propagated"))
                .ok_or_else(|| {
                    ApiError::bad("delivery_method must be direct, opportunistic or propagated")
                })
        })
        .transpose()?;
    let daemon_status = state.rpc("status", None).await?;
    let source = daemon_status
        .get("delivery_destination_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::unavailable("local LXMF delivery destination is unavailable"))?;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let id = hex::encode(bytes);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    let sent = state
        .rpc(
            "send_message_v2",
            Some(json!({
                "id":id,"source":source,"destination":destination,"title":"",
                "content":content,"fields":wire_fields,"method":method
            })),
        )
        .await
        .map_err(|error| {
            ApiError(StatusCode::SERVICE_UNAVAILABLE, format!("Sending Failed: {}", error.1))
        })?;
    let hash = sent
        .get("message_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::unavailable("daemon accepted send without a message id"))?;
    let live = json!({
        "hash":hash,"source_hash":source,"destination_hash":destination,"is_incoming":false,
        "state":"outbound","progress":0.0,"method":method.unwrap_or("direct"),
        "delivery_attempts":0,"next_delivery_attempt_at":null,"title":"","content":content,
        "fields":fields,"timestamp":timestamp,"rssi":null,"snr":null,"quality":null
    });
    state.publish(json!({"type":"lxmf_message_created","lxmf_message":live}));
    Ok(Json(json!({"lxmf_message":live})))
}

fn attachment_fields_for_wire(
    fields: &serde_json::Map<String, Value>,
    content_bytes: usize,
) -> Result<Value, ApiError> {
    if fields.keys().any(|key| !matches!(key.as_str(), "file_attachments" | "image")) {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            "Only file and image attachments are supported in the Rust proof of concept".into(),
        ));
    }
    let mut total = content_bytes;
    let mut wire = serde_json::Map::new();
    if let Some(files) = fields.get("file_attachments") {
        let files =
            files.as_array().ok_or_else(|| ApiError::bad("file_attachments must be an array"))?;
        if files.is_empty() {
            return Err(ApiError::bad("file_attachments must not be empty"));
        }
        let mut wire_files = Vec::with_capacity(files.len());
        for file in files {
            let name = file
                .get("file_name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty() && !name.contains(['/', '\\', '\0']))
                .ok_or_else(|| ApiError::bad("file_name must be a nonempty filename"))?;
            let encoded = file
                .get("file_bytes")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::bad("file_bytes must be base64"))?;
            let bytes =
                BASE64.decode(encoded).map_err(|_| ApiError::bad("file_bytes must be base64"))?;
            total = total.saturating_add(name.len()).saturating_add(bytes.len());
            wire_files.push(json!([name, bytes]));
        }
        wire.insert("5".into(), json!(wire_files));
    }
    if let Some(image) = fields.get("image") {
        let image_type = image
            .get("image_type")
            .and_then(Value::as_str)
            .filter(|kind| {
                !kind.is_empty()
                    && kind.len() <= 16
                    && kind.chars().all(|ch| ch.is_ascii_alphanumeric())
            })
            .ok_or_else(|| ApiError::bad("image_type must be a short image format name"))?;
        let encoded = image
            .get("image_bytes")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad("image_bytes must be base64"))?;
        let bytes =
            BASE64.decode(encoded).map_err(|_| ApiError::bad("image_bytes must be base64"))?;
        total = total.saturating_add(image_type.len()).saturating_add(bytes.len());
        wire.insert("6".into(), json!([image_type, bytes]));
    }
    if !wire.is_empty() && total > MAX_FILE_MESSAGE_BYTES {
        return Err(ApiError::bad(
            "Message with attachments exceeds the 900 KB proof of concept limit",
        ));
    }
    Ok(Value::Object(wire))
}

pub(super) async fn cancel_message(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult {
    state.rpc("sdk_cancel_message_v2", Some(json!({"message_id":hash}))).await?;
    let daemon = Arc::clone(&state.daemon);
    let row_hash = hash.clone();
    let row_id = tokio::task::spawn_blocking(move || daemon.meshchat_message_row_id(&row_hash))
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    let message = all_messages(&state, None)
        .await?
        .iter()
        .find(|record| record.get("id").and_then(Value::as_str) == Some(hash.as_str()))
        .map(|record| {
            let mut mapped = stored_message(record);
            mapped["id"] = json!(row_id);
            mapped
        });
    Ok(Json(json!({"message":"ok","lxmf_message":message})))
}

pub(super) async fn delete_message(
    State(state): State<Arc<AppState>>,
    Path(hash): Path<String>,
) -> ApiResult {
    let daemon = Arc::clone(&state.daemon);
    tokio::task::spawn_blocking(move || daemon.delete_meshchat_message(&hash))
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok(Json(json!({"message":"ok"})))
}

pub(super) async fn delete_conversation(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    let daemon = Arc::clone(&state.daemon);
    tokio::task::spawn_blocking(move || daemon.delete_meshchat_conversation(&destination))
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    Ok(Json(json!({"message":"ok"})))
}

pub(super) async fn history(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let after_id = query
        .get("after_id")
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| ApiError::bad("after_id must be an integer"))?;
    let count = query
        .get("count")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| ApiError::bad("count must be an integer"))?;
    let descending = query.get("order").is_some_and(|order| order != "asc");
    let daemon = Arc::clone(&state.daemon);
    let mapped = tokio::task::spawn_blocking(move || -> Result<Vec<Value>, ApiError> {
        let limit = count.unwrap_or(usize::MAX);
        let mut cursor = after_id;
        let mut messages = Vec::new();
        while messages.len() < limit {
            let page_size = (limit - messages.len()).min(100);
            let items = daemon
                .list_meshchat_conversation(&destination, cursor, descending, page_size)
                .map_err(|error| ApiError::unavailable(error.to_string()))?;
            let fetched = items.len();
            for (row_id, item) in items {
                cursor = Some(row_id);
                let record = serde_json::to_value(item)
                    .map_err(|error| ApiError::unavailable(error.to_string()))?;
                if is_command_only_message(&record) {
                    continue;
                }
                let mut message = stored_message(&record);
                message["id"] = json!(row_id);
                messages.push(message);
                if messages.len() == limit {
                    break;
                }
            }
            if fetched < page_size {
                break;
            }
        }
        Ok(messages)
    })
    .await
    .map_err(|error| ApiError::unavailable(error.to_string()))??;
    Ok(Json(json!({"lxmf_messages":mapped})))
}

pub(super) async fn conversations(State(state): State<Arc<AppState>>) -> ApiResult {
    let result = state.rpc("list_conversations", Some(json!({"limit":5000}))).await?;
    let entries = result
        .get("conversations")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::unavailable("daemon conversation list is malformed"))?;
    let local = state.local.read().await;
    let conversations: Vec<Value> = entries.iter().map(|entry| {
        let destination = entry.get("peer_destination_hex").and_then(Value::as_str).unwrap_or_default();
        // The legacy daemon field is named *_ms but is populated from its seconds timestamp.
        let updated = entry.get("last_message_at_ms").and_then(Value::as_u64).unwrap_or_default();
        let read = local.read_at.get(destination).copied().unwrap_or_default();
        json!({
            "destination_hash":destination,
            "display_name":entry.get("peer_display_name").and_then(Value::as_str).unwrap_or(destination),
            "custom_display_name":local.display_names.get(destination),
            "is_unread":entry.get("unread_count").and_then(Value::as_u64).unwrap_or_default()>0 && updated as i64>read,
            "failed_messages_count":0,"lxmf_user_icon":null,
            "updated_at":updated.to_string(),
        })
    }).collect();
    Ok(Json(json!({"conversations":conversations})))
}

pub(super) async fn mark_read(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let mut local = state.local.write().await;
    local.read_at.insert(destination, now);
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    Ok(Json(json!({"message":"ok"})))
}

async fn all_messages(state: &AppState, peer: Option<&str>) -> Result<Vec<Value>, ApiError> {
    let mut messages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let value = state
            .rpc("list_messages", Some(json!({"peer_id":peer,"limit":5000,"cursor":cursor})))
            .await?;
        let page = value
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::unavailable("daemon message history is malformed"))?;
        messages.extend(page.iter().cloned());
        let next = value.get("next_cursor").and_then(Value::as_str).map(str::to_owned);
        if next.is_none() || next == cursor {
            break;
        }
        cursor = next;
    }
    Ok(messages)
}

pub(super) fn stored_message(record: &Value) -> Value {
    let timestamp = record.get("timestamp").and_then(Value::as_i64).unwrap_or_default();
    let incoming = record.get("direction").and_then(Value::as_str) == Some("in");
    let status = record.get("receipt_status").and_then(Value::as_str).unwrap_or("");
    let state = if incoming { "delivered" } else { outbound_state(status) };
    let raw_fields = record.get("fields").filter(|value| value.is_object());
    let method =
        raw_fields.and_then(|fields| fields.pointer("/_lxmf/method")).and_then(Value::as_str);
    let mut fields = serde_json::Map::new();
    if let Some(object) = raw_fields.and_then(Value::as_object) {
        for key in ["image", "audio", "file_attachments"] {
            if let Some(value) = object.get(key) {
                fields.insert(key.to_owned(), value.clone());
            }
        }
        if let Some(files) = object.get("5").and_then(Value::as_array) {
            let decoded: Vec<Value> = files
                .iter()
                .filter_map(|entry| {
                    let pair = entry.as_array()?;
                    let name = pair.first()?.as_str()?;
                    Some(json!({"file_name":name,"file_bytes":BASE64.encode(json_bytes(pair.get(1)?)?)}))
                })
                .collect();
            if !decoded.is_empty() {
                fields.insert("file_attachments".into(), json!(decoded));
            }
        }
        if let Some(image) = object.get("6").and_then(Value::as_array) {
            if let (Some(Some(kind)), Some(Some(bytes))) =
                (image.first().map(Value::as_str), image.get(1).map(json_bytes))
            {
                fields.insert(
                    "image".into(),
                    json!({"image_type":kind,"image_bytes":BASE64.encode(bytes)}),
                );
            }
        }
        if let Some(audio) = object.get("7").and_then(Value::as_array) {
            if let (Some(Some(mode)), Some(Some(bytes))) =
                (audio.first().map(Value::as_u64), audio.get(1).map(json_bytes))
            {
                fields.insert(
                    "audio".into(),
                    json!({"audio_mode":mode,"audio_bytes":BASE64.encode(bytes)}),
                );
            }
        }
        if let Some(bytes) = object.get("2").and_then(json_bytes) {
            fields.insert("telemetry".into(), sideband_telemetry(&bytes));
        }
    }
    json!({
        "hash":record.get("id"),"source_hash":record.get("source"),
        "destination_hash":record.get("destination"),"is_incoming":incoming,
        "state":state,"progress":if state == "delivered" {100.0} else {0.0},
        "method":method,"delivery_attempts":0,"next_delivery_attempt_at":null,
        "failure_reason":status.strip_prefix("failed: "),
        "title":record.get("title").and_then(Value::as_str).unwrap_or(""),
        "content":record.get("content").and_then(Value::as_str).unwrap_or(""),
        "fields":fields,"timestamp":timestamp,"rssi":null,"snr":null,"quality":null,
        "id":0,"created_at":timestamp.to_string(),"updated_at":timestamp.to_string()
    })
}

fn sideband_telemetry(bytes: &[u8]) -> Value {
    let mut telemetry = json!({"raw_bytes":BASE64.encode(bytes),"sensor_ids":[]});
    let mut cursor = Cursor::new(bytes);
    if let Ok(rmpv::Value::Map(sensors)) = rmpv::decode::read_value(&mut cursor) {
        if cursor.position() == bytes.len() as u64 {
            let ids: Vec<u64> = sensors.iter().filter_map(|(key, _)| key.as_u64()).collect();
            let utc = sensors
                .iter()
                .find(|(key, _)| key.as_u64() == Some(1))
                .and_then(|(_, value)| value.as_i64());
            telemetry["sensor_ids"] = json!(ids);
            telemetry["utc"] = json!(utc);
            if let Some((_, rmpv::Value::Array(parts))) =
                sensors.iter().find(|(key, _)| key.as_u64() == Some(4))
            {
                if let (Some(charge), Some(charging)) = (
                    parts.first().and_then(rmpv::Value::as_f64),
                    parts.get(1).and_then(rmpv::Value::as_bool),
                ) {
                    telemetry["battery"] = json!({"charge_percent":charge,"charging":charging});
                }
            }
            if let Some((_, position)) = sensors.iter().find(|(key, _)| key.as_u64() == Some(2)) {
                telemetry["position"] = decode_sideband_position(position).unwrap_or(Value::Null);
            }
        }
    }
    telemetry
}

fn decode_sideband_position(value: &rmpv::Value) -> Option<Value> {
    let rmpv::Value::Array(parts) = value else {
        return None;
    };
    if parts.len() < 7 {
        return None;
    }
    let latitude = packed_i32(parts.first()?)? as f64 / 1e6;
    let longitude = packed_i32(parts.get(1)?)? as f64 / 1e6;
    let accuracy = packed_u16(parts.get(5)?)? as f64 / 1e2;
    let updated = parts.get(6).and_then(rmpv::Value::as_i64);
    Some(json!({"latitude":latitude,"longitude":longitude,"accuracy":accuracy,"updated":updated}))
}

fn packed_i32(value: &rmpv::Value) -> Option<i32> {
    let rmpv::Value::Binary(bytes) = value else {
        return None;
    };
    Some(i32::from_be_bytes(bytes.as_slice().try_into().ok()?))
}

fn packed_u16(value: &rmpv::Value) -> Option<u16> {
    let rmpv::Value::Binary(bytes) = value else {
        return None;
    };
    Some(u16::from_be_bytes(bytes.as_slice().try_into().ok()?))
}

pub(super) fn is_command_only_message(record: &Value) -> bool {
    let empty_text = ["title", "content"]
        .iter()
        .all(|key| record.get(*key).and_then(Value::as_str).is_none_or(str::is_empty));
    if !empty_text {
        return false;
    }
    let Some(fields) = record.get("fields").and_then(Value::as_object) else {
        return false;
    };
    let commands =
        fields.get("9").and_then(Value::as_array).is_some_and(|commands| !commands.is_empty());
    commands && fields.keys().all(|key| matches!(key.as_str(), "9" | "12" | "4" | "_lxmf"))
}

fn json_bytes(value: &Value) -> Option<Vec<u8>> {
    value
        .as_array()?
        .iter()
        .map(|byte| byte.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect()
}

pub(super) fn outbound_state(status: &str) -> &'static str {
    if status.starts_with("failed") {
        "failed"
    } else if status == "delivered" {
        "delivered"
    } else if status == "cancelled" {
        "cancelled"
    } else if status == "sending" {
        "sending"
    } else if status.starts_with("sent") {
        "sent"
    } else {
        "outbound"
    }
}
