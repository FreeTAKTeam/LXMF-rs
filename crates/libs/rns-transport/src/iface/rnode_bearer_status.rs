use std::sync::{Arc, Mutex};

use super::rnode_ble::{RnodeBleCommandMonitor, RnodeBleKissIoStats};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct RnodeBearerTraffic {
    pub(super) tx_packets: u64,
    pub(super) tx_bytes: u64,
    pub(super) rx_packets: u64,
    pub(super) rx_bytes: u64,
}

#[derive(Clone)]
pub struct RnodeBearerRuntimeStatusHandle {
    pub(super) inner: Arc<Mutex<serde_json::Value>>,
}

impl RnodeBearerRuntimeStatusHandle {
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }
}

pub(super) fn publish_monitor_status(
    status: &Arc<Mutex<serde_json::Value>>,
    monitor: &RnodeBleCommandMonitor,
    endpoint: &str,
    bearer: &str,
    negotiated_mtu: Option<u16>,
    traffic: RnodeBearerTraffic,
    io: RnodeBleKissIoStats,
) {
    let mut value = monitor.runtime_status_json(endpoint);
    if let Some(object) = value.as_object_mut() {
        object.insert("bearer".to_string(), serde_json::Value::String(bearer.to_string()));
        object.insert(
            "negotiated_mtu".to_string(),
            negotiated_mtu.map_or(serde_json::Value::Null, serde_json::Value::from),
        );
        object.insert(
            "traffic".to_string(),
            serde_json::json!({
                "tx_packets": traffic.tx_packets,
                "tx_bytes": traffic.tx_bytes,
                "rx_packets": traffic.rx_packets,
                "rx_bytes": traffic.rx_bytes,
                "kiss_write_chunks": io.write_chunks,
                "kiss_write_bytes": io.write_bytes,
                "kiss_read_chunks": io.read_chunks,
                "kiss_read_bytes": io.read_bytes,
            }),
        );
    }
    *status.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

pub(super) fn set_error_status(status: &Arc<Mutex<serde_json::Value>>, error: &str) {
    let mut guard = status.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(object) = guard.as_object_mut() else {
        return;
    };
    object.insert("online".to_string(), serde_json::Value::Bool(false));
    object.insert("last_command_error".to_string(), serde_json::Value::String(error.to_string()));
}

pub(super) fn append_error_status(status: &Arc<Mutex<serde_json::Value>>, error: &str) {
    let mut guard = status.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous = guard
        .get("last_command_error")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let Some(object) = guard.as_object_mut() else {
        return;
    };
    let combined = previous.map_or_else(|| error.to_string(), |value| format!("{value}; {error}"));
    object.insert("online".to_string(), serde_json::Value::Bool(false));
    object.insert("last_command_error".to_string(), serde_json::Value::String(combined));
}
