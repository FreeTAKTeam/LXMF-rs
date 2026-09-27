use crate::{chat, ApiError, ApiResult, AppState};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) async fn app_info(State(state): State<Arc<AppState>>) -> ApiResult {
    let daemon = state.rpc("daemon_status_ex", None).await?;
    let transport =
        daemon.pointer("/reticulum/transport/enabled").cloned().unwrap_or(Value::Bool(false));
    Ok(Json(json!({"app_info":{
        "backend":"rust-poc",
        "version": env!("CARGO_PKG_VERSION"), "lxmf_version": env!("CARGO_PKG_VERSION"),
        "rns_version": env!("CARGO_PKG_VERSION"), "python_version":"not applicable",
        "storage_path":"", "database_path":"", "database_file_size":0,
        "reticulum_config_path":"", "is_connected_to_shared_instance":false,
        "is_transport_enabled":transport
    }})))
}

pub(super) async fn config(State(state): State<Arc<AppState>>) -> ApiResult {
    Ok(Json(json!({"config": full_config(&state).await?})))
}

pub(super) async fn full_config(state: &AppState) -> Result<Value, ApiError> {
    let status = state.rpc("daemon_status_ex", None).await?;
    let local = state.local.read().await;
    let mut config = json!({
        "display_name":"Anonymous Peer",
        "identity_hash":status.get("identity_hash"),
        "lxmf_address_hash":status.get("delivery_destination_hash"),
        "audio_call_address_hash":"",
        "is_transport_enabled":status.pointer("/reticulum/transport/enabled").and_then(Value::as_bool).unwrap_or(false),
        "auto_announce_enabled":false,"auto_announce_interval_seconds":0,"last_announced_at":null,
        "theme":"light",
        "auto_resend_failed_messages_when_announce_received":false,
        "allow_auto_resending_failed_messages_with_attachments":false,
        "auto_send_failed_messages_to_propagation_node":false,
        "show_suggested_community_interfaces":true,
        "lxmf_local_propagation_node_enabled":status.pointer("/propagation/propagation_node_enabled").and_then(Value::as_bool).unwrap_or(false),
        "lxmf_local_propagation_node_address_hash":status.pointer("/propagation/destination_hash"),
        "lxmf_preferred_propagation_node_destination_hash":null,
        "lxmf_preferred_propagation_node_auto_sync_interval_seconds":0,
        "lxmf_preferred_propagation_node_last_synced_at":null,
        "lxmf_user_icon_name":null,"lxmf_user_icon_foreground_colour":null,
        "lxmf_user_icon_background_colour":null
    });
    if let Some(object) = config.as_object_mut() {
        object.extend(local.config.clone());
    }
    Ok(config)
}

pub(super) async fn patch_config(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    update_config(&state, body).await?;
    let config = full_config(&state).await?;
    state.publish(json!({"type":"config","config":config}));
    Ok(Json(json!({"config":config})))
}

async fn update_config(state: &AppState, body: Value) -> Result<(), ApiError> {
    let patch = body
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::bad("config object is required"))?;
    const LOCAL_FIELDS: &[&str] = &[
        "display_name",
        "theme",
        "show_suggested_community_interfaces",
        "lxmf_user_icon_name",
        "lxmf_user_icon_foreground_colour",
        "lxmf_user_icon_background_colour",
    ];
    const DAEMON_FIELDS: &[&str] = &[
        "auto_announce_interval_seconds",
        "auto_resend_failed_messages_when_announce_received",
        "allow_auto_resending_failed_messages_with_attachments",
        "auto_send_failed_messages_to_propagation_node",
        "lxmf_preferred_propagation_node_auto_sync_interval_seconds",
        "lxmf_local_propagation_node_enabled",
    ];
    if let Some(key) = patch.keys().find(|key| DAEMON_FIELDS.contains(&key.as_str())) {
        return Err(ApiError(
            StatusCode::NOT_IMPLEMENTED,
            format!("Configuration key {key} needs daemon integration"),
        ));
    }
    if let Some(key) = patch.keys().find(|key| {
        !LOCAL_FIELDS.contains(&key.as_str())
            && key.as_str() != "lxmf_preferred_propagation_node_destination_hash"
    }) {
        return Err(ApiError::bad(format!("Unknown configuration key {key}")));
    }
    let preferred = patch
        .get("lxmf_preferred_propagation_node_destination_hash")
        .map(preferred_propagation_node)
        .transpose()?;
    if let Some(peer) = &preferred {
        state.rpc("set_outbound_propagation_node", Some(json!({"peer":peer}))).await?;
    }
    let mut local = state.local.write().await;
    for (key, value) in patch {
        if key == "lxmf_preferred_propagation_node_destination_hash" {
            local
                .config
                .insert(key.clone(), json!(preferred.as_ref().expect("validated preference")));
        } else if LOCAL_FIELDS.contains(&key.as_str()) {
            local.config.insert(key.clone(), value.clone());
        }
    }
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await
}

pub(super) fn preferred_propagation_node(value: &Value) -> Result<Option<String>, ApiError> {
    match value {
        Value::Null => Ok(None),
        Value::String(peer) if peer.trim().is_empty() => Ok(None),
        Value::String(peer)
            if peer.len() == 32 && peer.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            Ok(Some(peer.to_ascii_lowercase()))
        }
        _ => Err(ApiError::bad("Propagation node destination hash must be 32 hex characters")),
    }
}

pub(super) async fn favourites(State(state): State<Arc<AppState>>) -> ApiResult {
    let local = state.local.read().await;
    let mut values: Vec<Value> = local.favourites.values().cloned().collect();
    values.sort_by(|a, b| a["id"].as_u64().cmp(&b["id"].as_u64()));
    Ok(Json(json!({"favourites":values})))
}

pub(super) async fn add_favourite(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    let favourite = body.get("favourite").unwrap_or(&body);
    let destination = favourite
        .get("destination_hash")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("destination_hash is required"))?;
    let name = favourite
        .get("display_name")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("display_name is required"))?;
    let aspect = favourite.get("aspect").and_then(Value::as_str).unwrap_or("lxmf.delivery");
    let now = now_seconds().to_string();
    let mut local = state.local.write().await;
    let id = local
        .favourites
        .values()
        .filter_map(|item| item.get("id").and_then(Value::as_u64))
        .max()
        .unwrap_or_default()
        + 1;
    let saved = json!({"id":id,"destination_hash":destination,"display_name":name,
        "aspect":aspect,"created_at":now,"updated_at":now});
    local.favourites.insert(destination.to_owned(), saved.clone());
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    Ok(Json(json!({"message":"Favourite has been added!"})))
}

pub(super) async fn rename_favourite(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let name = body
        .get("display_name")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("display_name is required"))?;
    let mut local = state.local.write().await;
    let saved = local
        .favourites
        .get_mut(&destination)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "Favourite not found".into()))?;
    saved["display_name"] = json!(name);
    saved["updated_at"] = json!(now_seconds().to_string());
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    Ok(Json(json!({"message":"Favourite has been renamed"})))
}

pub(super) async fn delete_favourite(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    let mut local = state.local.write().await;
    local.favourites.remove(&destination);
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    Ok(Json(json!({"message":"Favourite deleted"})))
}

pub(super) async fn display_name(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    let name = state.local.read().await.display_names.get(&destination).cloned();
    Ok(Json(json!({"custom_display_name":name})))
}

pub(super) async fn update_display_name(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult {
    let name = body.get("display_name").and_then(Value::as_str).map(str::to_owned);
    let mut local = state.local.write().await;
    if let Some(name) = &name.filter(|name| !name.is_empty()) {
        local.display_names.insert(destination, name.clone());
    } else {
        local.display_names.remove(&destination);
    }
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    Ok(Json(json!({"message":"Custom display name updated"})))
}

pub(super) async fn announces(State(state): State<Arc<AppState>>) -> ApiResult {
    let value = state.rpc("list_announces", Some(json!({"limit":5000}))).await?;
    let entries = value
        .get("announces")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::unavailable("daemon announce list is malformed"))?;
    let local = state.local.read().await;
    let mapped: Vec<Value> = entries.iter().enumerate().map(|(index, entry)| {
        let destination = entry.get("peer").and_then(Value::as_str).unwrap_or_default();
        let timestamp = entry.get("timestamp").and_then(Value::as_i64).unwrap_or_default().to_string();
        let app_data = entry.get("app_data_hex").and_then(Value::as_str)
            .and_then(|hex| hex::decode(hex).ok()).map(|bytes| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            });
        let first_seen = entry.get("first_seen").and_then(Value::as_i64).unwrap_or_default().to_string();
        json!({"id":index+1,"destination_hash":destination,"app_data":app_data,
            "rssi":entry.get("rssi"),"snr":entry.get("snr"),"quality":entry.get("q"),
            "display_name":entry.get("name"),"custom_display_name":local.display_names.get(destination),
            "lxmf_user_icon":null,"created_at":first_seen,"updated_at":timestamp})
    }).collect();
    Ok(Json(json!({"announces":mapped})))
}

pub(super) async fn websocket(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.max_message_size(52_428_800).on_upgrade(move |socket| socket_loop(socket, state))
}

async fn socket_loop(socket: WebSocket, state: Arc<AppState>) {
    let mut local_events = state.events.subscribe();
    let mut daemon_events = state.daemon.subscribe_events();
    if let Ok(config) = full_config(&state).await {
        state.publish(json!({"type":"config","config":config}));
    }
    let (mut sender, mut receiver) = socket.split();
    loop {
        tokio::select! {
            message = receiver.next() => {
                let Some(Ok(message)) = message else { break; };
                if let Message::Text(text) = message {
                    let Ok(value) = serde_json::from_str::<Value>(&text) else { continue; };
                    match value.get("type").and_then(Value::as_str) {
                        Some("ping") => {
                            if sender.send(Message::Text(json!({"type":"pong"}).to_string().into())).await.is_err() { break; }
                        }
                        Some("config.set") => {
                            if let Some(config) = value.get("config") {
                                if update_config(&state, json!({"config":config})).await.is_ok() {
                                    if let Ok(config) = full_config(&state).await {
                                        state.publish(json!({"type":"config","config":config}));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            event = local_events.recv() => {
                if let Ok(value) = event {
                    if sender.send(Message::Text(value.to_string().into())).await.is_err() { break; }
                }
            }
            event = daemon_events.recv() => {
                if let Ok(event) = event {
                    let mapped = match event.event_type.as_str() {
                        "inbound" => {
                            let record = event.payload.get("message");
                            if record.is_some_and(chat::is_command_only_message) {
                                continue;
                            }
                            if let Some(hash) = record.and_then(|record| record.get("id")).and_then(Value::as_str) {
                                let daemon = Arc::clone(&state.daemon);
                                let hash = hash.to_owned();
                                let row_id = tokio::task::spawn_blocking(move || daemon.meshchat_message_row_id(&hash)).await;
                                match (record, row_id) {
                                    (Some(record), Ok(Ok(Some(row_id)))) => {
                                        let mut message = chat::stored_message(record);
                                        message["id"] = json!(row_id);
                                        Some(json!({"type":"lxmf.delivery","lxmf_message":message}))
                                    }
                                    _ => {
                                        log::warn!("[meshchat-api] inbound event has no stored message row id");
                                        None
                                    }
                                }
                            } else { None }
                        }
                        "outbound" => event.payload.get("message").map(|record| json!({"type":"lxmf_message_state_updated","lxmf_message":chat::stored_message(record)})),
                        "receipt" => {
                            let hash = event.payload.get("message_id").and_then(Value::as_str);
                            let status = event.payload.get("status").and_then(Value::as_str);
                            match (hash, status) {
                                (Some(hash), Some(status)) => {
                                    let state = chat::outbound_state(status);
                                    Some(json!({"type":"lxmf_message_state_updated","lxmf_message":{
                                        "hash":hash,"state":state,
                                        "progress":if state == "delivered" {100.0} else {0.0}
                                    }}))
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(value) = mapped {
                        if sender.send(Message::Text(value.to_string().into())).await.is_err() { break; }
                    }
                }
            }
        }
    }
}

pub(super) fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_secs()).unwrap_or_default()
}
