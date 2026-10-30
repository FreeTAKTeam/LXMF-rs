mod discovery;
mod exchange;
mod store;
mod write;

pub(super) use store::ConfigStore;
use write::save_interface;

use crate::{ApiError, ApiResult, AppState};
use axum::{extract::State, Json};
use serde_json::{json, Value};
use std::sync::Arc;
use toml_edit::{Item, Table};

fn config_task<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce(Arc<ConfigStore>) -> Result<T, ApiError> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, ApiError>> {
    let store = state.config.clone();
    async move {
        let store = store.ok_or_else(|| {
            ApiError::unavailable(
                "Start reticulumd with --config to manage saved interfaces and transport",
            )
        })?;
        tokio::task::spawn_blocking(move || operation(store))
            .await
            .map_err(|error| ApiError::unavailable(error.to_string()))?
    }
}

pub(super) async fn interfaces(State(state): State<Arc<AppState>>) -> ApiResult {
    let interfaces = config_task(&state, |store| store.read(store::saved_interfaces)).await?;
    Ok(Json(json!({"interfaces":interfaces})))
}

pub(super) async fn comports() -> ApiResult {
    let ports = tokio::task::spawn_blocking(serialport::available_ports)
        .await
        .map_err(|error| ApiError::unavailable(error.to_string()))?
        .map_err(|error| ApiError::unavailable(format!("Cannot list serial ports: {error}")))?;
    let comports = ports
        .into_iter()
        .map(|port| {
            let (product, serial_number) = match port.port_type {
                serialport::SerialPortType::UsbPort(info) => (info.product, info.serial_number),
                _ => (None, None),
            };
            json!({"device":port.port_name,"product":product,"serial_number":serial_number})
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"comports":comports})))
}

pub(super) async fn interface_stats(State(state): State<Arc<AppState>>) -> ApiResult {
    let status = state.rpc("daemon_status_ex", None).await?;
    let records = status
        .get("interfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::unavailable("daemon interface status is malformed"))?;
    let entries: Vec<Value> = records
        .iter()
        .map(|record| {
            let mut entry = record.clone();
            if let Some(object) = entry.as_object_mut() {
                if let Some(name) = object.get("name").cloned() {
                    object.insert("interface_name".into(), name);
                }
            }
            entry
        })
        .collect();
    Ok(Json(json!({"interface_stats":{
        "interfaces":entries,
        "transport_id":status.pointer("/reticulum/transport/transport_id"),
        "network_id":status.pointer("/reticulum/network_id"),
        "probe_responder":status.pointer("/reticulum/probe_responder")
    }})))
}

fn required_name(body: &Value) -> Result<&str, ApiError> {
    body.get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ApiError::bad("Name is required"))
}

pub(super) async fn enable(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    toggle_interface(&state, body, true).await
}

pub(super) async fn disable(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    toggle_interface(&state, body, false).await
}

async fn toggle_interface(state: &AppState, body: Value, enabled: bool) -> ApiResult {
    let name = required_name(&body)?.to_string();
    config_task(state, move |store| {
        store.edit(|doc| {
            let record = store::interface_mut(doc, &name)
                .ok_or_else(|| ApiError::bad(format!("Interface {name} was not found")))?;
            // The two aliases are both accepted by reticulumd. Add the canonical one if neither exists.
            let mut found = false;
            for key in ["enabled", "interface_enabled"] {
                if record.contains_key(key) {
                    record[key] = toml_edit::value(enabled);
                    found = true;
                }
            }
            if !found {
                record["interface_enabled"] = toml_edit::value(enabled);
            }
            Ok(())
        })
    })
    .await?;
    Ok(Json(
        json!({"message":if enabled { "Interface is now enabled" } else { "Interface is now disabled" }}),
    ))
}

pub(super) async fn delete(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    let name = required_name(&body)?.to_string();
    config_task(&state, move |store| {
        store.edit(|doc| {
            if !store::remove_interface(doc, &name)? {
                return Err(ApiError::bad(format!("Interface {name} was not found")));
            }
            Ok(())
        })
    })
    .await?;
    Ok(Json(json!({"message":"Interface has been deleted"})))
}

pub(super) async fn add(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let overwrite =
        body.get("allow_overwriting_interface").and_then(Value::as_bool).unwrap_or(false);
    let name = required_name(&body)
        .map_err(|_| {
            ApiError(axum::http::StatusCode::UNPROCESSABLE_ENTITY, "Name is required".into())
        })?
        .to_string();
    config_task(&state, move |store| {
        store.edit(|doc| save_interface(doc, &name, &body, overwrite))
    })
    .await?;
    let message = if overwrite {
        "Interface has been saved. Please restart MeshChat for these changes to take effect."
    } else {
        "Interface has been added. Please restart MeshChat for these changes to take effect."
    };
    Ok(Json(json!({"message":message})))
}

pub(super) async fn enable_transport(State(state): State<Arc<AppState>>) -> ApiResult {
    toggle_transport(&state, true).await
}

pub(super) async fn disable_transport(State(state): State<Arc<AppState>>) -> ApiResult {
    toggle_transport(&state, false).await
}

async fn toggle_transport(state: &AppState, enabled: bool) -> ApiResult {
    set_reticulum_flag(state, "enable_transport", enabled).await?;
    Ok(Json(json!({"message":if enabled {
        "Transport has been enabled. MeshChat must be restarted for this change to take effect."
    } else {
        "Transport has been disabled. MeshChat must be restarted for this change to take effect."
    }})))
}

async fn set_reticulum_flag(
    state: &AppState,
    key: &'static str,
    enabled: bool,
) -> Result<(), ApiError> {
    config_task(state, move |store| {
        store.edit(|doc| {
            if doc.get("reticulum").is_none() {
                doc["reticulum"] = Item::Table(Table::new());
            }
            let table = doc["reticulum"]
                .as_table_mut()
                .ok_or_else(|| ApiError::unavailable("reticulum config is not a table"))?;
            table[key] = toml_edit::value(enabled);
            Ok(())
        })
    })
    .await
}

pub(super) use discovery::{disable_discovery, discovered_interfaces, enable_discovery};
pub(super) use exchange::{export, import, import_preview};
