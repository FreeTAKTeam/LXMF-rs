//! Loopback MeshChat compatibility API backed by the running Reticulum daemon.

mod chat;
mod control;
#[cfg(test)]
mod tests;

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use rns_rpc::{RpcDaemon, RpcRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tower_http::services::ServeDir;

const MAX_BODY_BYTES: usize = 52_428_800;

#[derive(Clone)]
pub struct AppState {
    daemon: Arc<RpcDaemon>,
    local: Arc<RwLock<LocalState>>,
    events: broadcast::Sender<Value>,
    persist: Option<mpsc::UnboundedSender<(LocalState, oneshot::Sender<std::io::Result<()>>)>>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct LocalState {
    config: serde_json::Map<String, Value>,
    favourites: HashMap<String, Value>,
    display_names: HashMap<String, String>,
    read_at: HashMap<String, i64>,
}

#[derive(Debug)]
struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    fn unavailable(message: impl Into<String>) -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, message.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"message": self.1}))).into_response()
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

impl AppState {
    async fn rpc(&self, method: &'static str, params: Option<Value>) -> Result<Value, ApiError> {
        let daemon = Arc::clone(&self.daemon);
        let result = tokio::task::spawn_blocking(move || {
            daemon.handle_rpc(RpcRequest { id: 1, method: method.to_owned(), params })
        })
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
        if let Some(error) = result.error {
            return Err(ApiError::unavailable(error.message));
        }
        Ok(result.result.unwrap_or(Value::Null))
    }

    fn publish(&self, event: Value) {
        if self.events.send(event).is_err() {
            log::trace!("[meshchat] event had no subscribers");
        }
    }

    fn queue_persist(
        &self,
        local: &LocalState,
    ) -> Result<Option<oneshot::Receiver<std::io::Result<()>>>, ApiError> {
        let Some(sender) = &self.persist else {
            return Ok(None);
        };
        let (done, receiver) = oneshot::channel();
        sender
            .send((local.clone(), done))
            .map_err(|_| ApiError::unavailable("MeshChat state writer stopped"))?;
        Ok(Some(receiver))
    }

    async fn wait_persist(
        receiver: Option<oneshot::Receiver<std::io::Result<()>>>,
    ) -> Result<(), ApiError> {
        if let Some(receiver) = receiver {
            receiver
                .await
                .map_err(|_| ApiError::unavailable("MeshChat state writer stopped"))?
                .map_err(|error| {
                    ApiError::unavailable(format!("MeshChat state write failed: {error}"))
                })?;
        }
        Ok(())
    }
}

pub fn router(daemon: Arc<RpcDaemon>, assets: Option<PathBuf>) -> Router {
    let (events, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        daemon,
        local: Arc::new(RwLock::new(LocalState::default())),
        events,
        persist: None,
    });
    build_router(state, assets)
}

fn build_router(state: Arc<AppState>, assets: Option<PathBuf>) -> Router {
    let mut app = Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/app/info", get(control::app_info))
        .route("/api/v1/config", get(control::config).patch(control::patch_config))
        .route("/ws", get(control::websocket))
        .route("/api/v1/announce", get(announce))
        .route("/api/v1/announces", get(control::announces))
        .route("/api/v1/favourites", get(control::favourites))
        .route("/api/v1/favourites/add", post(control::add_favourite))
        .route("/api/v1/favourites/{destination_hash}/rename", post(control::rename_favourite))
        .route("/api/v1/favourites/{destination_hash}", delete(control::delete_favourite))
        .route("/api/v1/lxmf-messages/send", post(chat::send_message))
        .route("/api/v1/lxmf-messages/{hash}/cancel", post(chat::cancel_message))
        .route("/api/v1/lxmf-messages/{hash}", delete(chat::delete_message))
        .route(
            "/api/v1/lxmf-messages/conversation/{destination_hash}",
            get(chat::history).delete(chat::delete_conversation),
        )
        .route("/api/v1/lxmf/conversations", get(chat::conversations))
        .route("/api/v1/lxmf/conversations/{destination_hash}/mark-as-read", get(chat::mark_read))
        .route("/api/v1/destination/{destination_hash}/path", get(path_status))
        .route("/api/v1/destination/{destination_hash}/drop-path", post(drop_path))
        .route(
            "/api/v1/destination/{destination_hash}/custom-display-name",
            get(control::display_name),
        )
        .route(
            "/api/v1/destination/{destination_hash}/custom-display-name/update",
            post(control::update_display_name),
        )
        .route("/api/v1/lxmf/propagation-node/status", get(propagation_status))
        .route("/api/v1/lxmf/propagation-nodes", get(propagation_nodes))
        .route("/api/v1/lxmf/propagation-node/sync", get(sync_propagation_node))
        .route("/api/v1/lxmf/propagation-node/stop-sync", get(unsupported))
        .route("/api/v1/destination/{destination_hash}/signal-metrics", get(signal_metrics))
        .route("/api/v1/destination/{destination_hash}/lxmf-stamp-info", get(stamp_info))
        .route("/api/v1/ping/{destination_hash}/lxmf.delivery", get(unsupported))
        .route("/api/v1/interface-stats", get(unsupported))
        .route("/api/v1/path-table", get(unsupported))
        .route("/api/v1/comports", get(unsupported))
        .route("/api/v1/reticulum/interfaces", get(interfaces))
        .route("/api/v1/reticulum/interfaces/enable", post(unsupported))
        .route("/api/v1/reticulum/interfaces/disable", post(unsupported))
        .route("/api/v1/reticulum/interfaces/delete", post(unsupported))
        .route("/api/v1/reticulum/interfaces/add", post(unsupported))
        .route("/api/v1/reticulum/interfaces/export", post(unsupported))
        .route("/api/v1/reticulum/interfaces/import-preview", post(unsupported))
        .route("/api/v1/reticulum/interfaces/import", post(unsupported))
        .route("/api/v1/reticulum/enable-transport", post(unsupported))
        .route("/api/v1/reticulum/disable-transport", post(unsupported))
        .route("/api/v1/calls", get(calls))
        .route("/api/v1/calls/clear-call-history", post(unsupported))
        .route("/api/v1/calls/hangup-all", get(unsupported))
        .route("/api/v1/calls/initiate/{destination_hash}", get(unsupported))
        .route("/api/v1/calls/{audio_call_link_hash}/audio", get(unsupported))
        .route("/api/v1/calls/{audio_call_link_hash}/hangup", get(unsupported))
        .route("/api/v1/calls/{audio_call_link_hash}", get(unsupported).delete(unsupported))
        .route("/api/v1/nomadnetwork/{destination_hash}/identify", post(unsupported))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));
    if let Some(assets) = assets {
        app = app.fallback_service(ServeDir::new(assets));
    }
    app.with_state(state)
}

pub async fn serve(
    bind: SocketAddr,
    daemon: Arc<RpcDaemon>,
    assets: Option<PathBuf>,
    state_path: PathBuf,
) -> std::io::Result<()> {
    if !bind.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "MeshChat API requires a loopback bind",
        ));
    }
    let local = match tokio::fs::read(&state_path).await {
        Ok(bytes) => serde_json::from_slice::<LocalState>(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LocalState::default(),
        Err(error) => return Err(error),
    };
    let (persist_tx, mut persist_rx) =
        mpsc::unbounded_channel::<(LocalState, oneshot::Sender<std::io::Result<()>>)>();
    tokio::spawn(async move {
        while let Some((state, done)) = persist_rx.recv().await {
            let result = persist_state(&state_path, &state).await;
            match done.send(result) {
                Ok(()) => {}
                Err(Ok(())) => log::debug!("[meshchat] state persisted after requester left"),
                Err(Err(error)) => {
                    log::warn!("[meshchat] state persistence failed after requester left: {error}");
                }
            }
        }
    });
    let (events, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        daemon,
        local: Arc::new(RwLock::new(local)),
        events,
        persist: Some(persist_tx),
    });
    if let Some(value) = state
        .local
        .read()
        .await
        .config
        .get("lxmf_preferred_propagation_node_destination_hash")
        .cloned()
    {
        let peer = control::preferred_propagation_node(&value)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.1))?;
        state
            .rpc("set_outbound_propagation_node", Some(json!({"peer":peer})))
            .await
            .map_err(|error| std::io::Error::other(error.1))?;
    }
    let listener = tokio::net::TcpListener::bind(bind).await?;
    log::info!("MeshChat API listening on http://{}", listener.local_addr()?);
    axum::serve(listener, build_router(state, assets)).await
}

async fn persist_state(path: &std::path::Path, state: &LocalState) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(state).map_err(std::io::Error::other)?;
    let temp = path.with_extension("meshchat.tmp");
    tokio::fs::write(&temp, bytes).await?;
    tokio::fs::rename(temp, path).await
}

async fn unsupported() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({"message":"This MeshChat operation is not supported by the Reticulum daemon"})),
    )
}

async fn interfaces(State(state): State<Arc<AppState>>) -> ApiResult {
    let status = state.rpc("daemon_status_ex", None).await?;
    let records = status
        .get("interfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::unavailable("daemon interface status is malformed"))?;
    let interfaces: Vec<Value> = records
        .iter()
        .map(|record| {
            let connection = record
                .pointer("/settings/_runtime/tcp/stream_status/stream_state")
                .cloned()
                .or_else(|| {
                    record
                        .pointer("/settings/_runtime/lora/rnode_status/online")
                        .and_then(Value::as_bool)
                        .map(|online| json!(if online { "online" } else { "offline" }))
                });
            json!({
                "name":record.get("name"),
                "type":record.get("type"),
                "enabled":record.get("enabled"),
                "connection":connection,
            })
        })
        .collect();
    Ok(Json(json!({"interfaces":interfaces})))
}

async fn status(State(state): State<Arc<AppState>>) -> ApiResult {
    state.rpc("status", None).await?;
    Ok(Json(json!({"status":"ok"})))
}

async fn announce(State(state): State<Arc<AppState>>) -> ApiResult {
    state.rpc("announce_now", None).await?;
    let mut local = state.local.write().await;
    local.config.insert("last_announced_at".into(), json!(control::now_seconds()));
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    state.publish(json!({"type":"announced"}));
    Ok(Json(json!({"message":"announcing"})))
}

async fn sync_propagation_node(State(state): State<Arc<AppState>>) -> ApiResult {
    let peer = {
        let local = state.local.read().await;
        local
            .config
            .get("lxmf_preferred_propagation_node_destination_hash")
            .map(control::preferred_propagation_node)
            .transpose()?
            .flatten()
    }
    .ok_or_else(|| ApiError::bad("A propagation node must be configured to sync messages."))?;
    let result = state
        .rpc(
            "propagation_remote_fetch",
            Some(json!({
                "remote":peer,
                "timeout_secs":60.0,
                "transfer_limit_kb":1024.0
            })),
        )
        .await?;
    if result.pointer("/result/postponed").and_then(Value::as_bool) == Some(true) {
        return Err(ApiError::unavailable("Propagation fetch was postponed"));
    }
    let received =
        result.pointer("/result/local_imported_count").and_then(Value::as_u64).unwrap_or(0);
    let mut local = state.local.write().await;
    local.config.insert(
        "lxmf_preferred_propagation_node_last_synced_at".into(),
        json!(control::now_seconds()),
    );
    let persisted = state.queue_persist(&local)?;
    drop(local);
    AppState::wait_persist(persisted).await?;
    if let Ok(config) = control::full_config(&state).await {
        state.publish(json!({"type":"config","config":config}));
    }
    Ok(Json(json!({"message":"Sync complete","messages_received":received})))
}

async fn path_status(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    let request = matches!(query.get("request").map(String::as_str), Some("true" | "1"));
    if request {
        state.rpc("request_path", Some(json!({"destination":destination}))).await?;
    }
    let value = match state.rpc("path_status", Some(json!({"destination":destination}))).await {
        Ok(value) => value,
        Err(ApiError(_, message)) if message == "path lookup bridge is not configured" => {
            return Ok(Json(json!({"path":null})))
        }
        Err(error) => return Err(error),
    };
    if !value.get("known").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(Json(json!({"path":null})));
    }
    Ok(Json(json!({"path":{
        "hops":value.get("hops"),
        "next_hop":value.get("next_hop"),
        "next_hop_interface":value.get("next_hop_interface")
    }})))
}

async fn drop_path(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    state.rpc("drop_path", Some(json!({"destination":destination}))).await?;
    Ok(Json(json!({"message":"Path has been dropped"})))
}

async fn propagation_status(State(state): State<Arc<AppState>>) -> ApiResult {
    let value = state.rpc("propagation_status", None).await?;
    let source = value
        .get("propagation")
        .ok_or_else(|| ApiError::unavailable("daemon propagation status is malformed"))?;
    let state_name = source.get("state_name").and_then(Value::as_str).unwrap_or("");
    let mapped = match state_name {
        "" => "idle",
        "syncing" => "request_sent",
        "completed" => "complete",
        "downloading" | "fetching" => "receiving",
        "no_identity" => "no_identity_received",
        known @ ("idle" | "path_requested" | "link_establishing" | "link_established"
        | "receiving" | "response_received" | "no_path" | "link_failed"
        | "transfer_failed" | "no_access" | "failed") => known,
        _ => "unknown",
    };
    let progress = source.get("sync_progress").and_then(Value::as_f64).unwrap_or_default() * 100.0;
    Ok(Json(json!({"propagation_node_status":{
        "state":mapped,"progress":progress,"messages_received":source.get("messages_received")
    }})))
}

async fn calls() -> ApiResult {
    // No MeshChat audio-call manager is attached to this daemon runtime.
    Ok(Json(json!({"audio_calls":[]})))
}

async fn stamp_info(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    let value =
        state.rpc("get_outbound_stamp_cost", Some(json!({"destination":destination}))).await?;
    let daemon = Arc::clone(&state.daemon);
    let expiry = tokio::task::spawn_blocking(move || daemon.outbound_ticket_for(&destination))
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map(|ticket| ticket.expires_at);
    Ok(Json(json!({"lxmf_stamp_info":{
        "stamp_cost":value.get("stamp_cost"),
        "outbound_ticket_expiry":expiry
    }})))
}

async fn signal_metrics(
    State(state): State<Arc<AppState>>,
    Path(destination): Path<String>,
) -> ApiResult {
    let announces = state.rpc("list_announces", Some(json!({"limit":5000}))).await?;
    let announce = announces.get("announces").and_then(Value::as_array).and_then(|items| {
        items
            .iter()
            .filter(|item| item.get("peer").and_then(Value::as_str) == Some(destination.as_str()))
            .max_by_key(|item| item.get("timestamp").and_then(Value::as_i64).unwrap_or_default())
    });
    let messages =
        state.rpc("list_messages", Some(json!({"peer_id":destination,"limit":5000}))).await?;
    let message = messages.get("messages").and_then(Value::as_array).and_then(|items| {
        items
            .iter()
            .filter(|item| item.get("direction").and_then(Value::as_str) == Some("in"))
            .max_by_key(|item| item.get("timestamp").and_then(Value::as_i64).unwrap_or_default())
    });
    let record = match (announce, message) {
        (Some(announce), Some(message))
            if message.get("timestamp").and_then(Value::as_i64)
                > announce.get("timestamp").and_then(Value::as_i64) =>
        {
            Some(message)
        }
        (Some(announce), _) => Some(announce),
        (None, message) => message,
    };
    Ok(Json(json!({"signal_metrics":{
        "rssi":record.and_then(|item| item.get("rssi")),
        "snr":record.and_then(|item| item.get("snr")),
        "quality":record.and_then(|item| item.get("q").or_else(|| item.get("quality"))),
        "updated_at":record.and_then(|item| item.get("timestamp")).map(Value::to_string)
    }})))
}

async fn propagation_nodes(State(state): State<Arc<AppState>>) -> ApiResult {
    let value = state.rpc("list_propagation_nodes", None).await?;
    let nodes = value
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::unavailable("daemon propagation node list is malformed"))?;
    let mapped: Vec<Value> = nodes
        .iter()
        .map(|node| {
            json!({
                "destination_hash":node.get("peer"),
                "operator_display_name":node.get("name"),
                "is_propagation_enabled":true,
                "updated_at":node.get("last_seen"),
            })
        })
        .collect();
    Ok(Json(json!({"lxmf_propagation_nodes":mapped})))
}
