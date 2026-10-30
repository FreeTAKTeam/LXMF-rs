//! Interface discovery for MeshChat-compatible clients: what the daemon has
//! heard announced, and the saved `discover_interfaces` policy.

use super::{config_task, set_reticulum_flag};
use crate::{ApiError, ApiResult, AppState};
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;
use toml_edit::DocumentMut;

pub(crate) async fn discovered_interfaces(State(state): State<Arc<AppState>>) -> ApiResult {
    // Without --config there is no saved policy to report; the list itself still works.
    let discovery_enabled = config_task(&state, |store| store.read(|doc| Ok(discovery_saved(doc))))
        .await
        .unwrap_or(false);
    let rows = state.rpc("discovered_interfaces", None).await?;
    let rows = rows
        .as_array()
        .ok_or_else(|| ApiError::unavailable("daemon discovered interface list is malformed"))?;
    let interfaces: Vec<Value> = rows.iter().map(discovered_interface).collect();
    Ok(Json(json!({"discovery_enabled":discovery_enabled, "interfaces":interfaces})))
}

pub(crate) async fn enable_discovery(State(state): State<Arc<AppState>>) -> ApiResult {
    toggle_discovery(&state, true).await
}

pub(crate) async fn disable_discovery(State(state): State<Arc<AppState>>) -> ApiResult {
    toggle_discovery(&state, false).await
}

async fn toggle_discovery(state: &AppState, enabled: bool) -> ApiResult {
    set_reticulum_flag(state, "discover_interfaces", enabled).await?;
    Ok(Json(json!({"message":if enabled {
        "Interface discovery has been enabled. Restart the daemon for this change to take effect."
    } else {
        "Interface discovery has been disabled. Restart the daemon for this change to take effect."
    }})))
}

fn discovery_saved(doc: &DocumentMut) -> bool {
    doc.get("reticulum")
        .and_then(|table| table.get("discover_interfaces"))
        .and_then(toml_edit::Item::as_bool)
        .unwrap_or(false)
}

fn discovered_interface(row: &Value) -> Value {
    let discovery_hash = row.get("discovery_hash").and_then(Value::as_array).map(|bytes| {
        bytes.iter().filter_map(Value::as_u64).map(|byte| format!("{byte:02x}")).collect::<String>()
    });
    json!({
        "discovery_hash":discovery_hash,
        "name":row.get("name"),
        "type":row.get("type"),
        "transport":row.get("transport"),
        "status":row.get("status"),
        "reachable_on":row.get("reachable_on"),
        "port":row.get("port"),
        "hops":row.get("hops"),
        "network_id":row.get("network_id"),
        "transport_id":row.get("transport_id"),
        "ifac_netname":row.get("ifac_netname"),
        "ifac_netkey":row.get("ifac_netkey"),
        "latitude":row.get("latitude"),
        "longitude":row.get("longitude"),
        "operator_lxmf_address":row.get("operator_lxmf_address"),
        "last_heard":row.get("last_heard"),
        "discovered":row.get("discovered"),
    })
}
