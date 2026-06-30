use super::super::with_interface_runtime_metadata;
use rns_rpc::InterfaceRecord;
use serde_json::{json, Value as JsonValue};

#[derive(Clone, Debug)]
pub(super) enum PathTableRestoreStatus {
    Ok { restored_active_paths: usize },
    Error { message: String },
}

impl PathTableRestoreStatus {
    fn runtime_json(&self) -> JsonValue {
        match self {
            Self::Ok { restored_active_paths } => {
                json!({
                    "status": "ok",
                    "restored_active_paths": restored_active_paths,
                })
            }
            Self::Error { message } => {
                json!({
                    "status": "error",
                    "error": message,
                })
            }
        }
    }
}

pub(super) fn mark_path_table_restore_status(
    record: &mut InterfaceRecord,
    status: &PathTableRestoreStatus,
) {
    with_interface_runtime_metadata(record, |runtime| {
        let reticulum = runtime.entry("reticulum".to_string()).or_insert_with(|| json!({}));
        if !reticulum.is_object() {
            *reticulum = json!({});
        }
        if let Some(reticulum) = reticulum.as_object_mut() {
            reticulum.insert("path_table_restore".to_string(), status.runtime_json());
        }
    });
}

pub(super) fn mark_path_table_restore_status_on_enabled_interfaces(
    records: &mut [InterfaceRecord],
    status: &PathTableRestoreStatus,
) {
    for record in records {
        if record.enabled {
            mark_path_table_restore_status(record, status);
        }
    }
}
