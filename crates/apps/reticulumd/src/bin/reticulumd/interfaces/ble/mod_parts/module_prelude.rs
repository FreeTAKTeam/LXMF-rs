use reticulum_daemon::config::InterfaceConfig;

use rns_transport::iface::InterfaceManager;

use std::sync::{Arc, Mutex};

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BleRuntimeStatus {
    pub(crate) link_state: String,
    pub(crate) adapter: Option<String>,
    pub(crate) peripheral_id: String,
    pub(crate) service_uuid: String,
    pub(crate) write_char_uuid: String,
    pub(crate) notify_char_uuid: String,
    pub(crate) mtu: usize,
    pub(crate) scan_timeout_ms: u64,
    pub(crate) connect_timeout_ms: u64,
    pub(crate) iface: Option<String>,
    pub(crate) connected: bool,
    pub(crate) subscribed: bool,
    pub(crate) reconnect_attempts: u64,
    pub(crate) scan_errors: u64,
    pub(crate) connect_errors: u64,
    pub(crate) subscribe_errors: u64,
    pub(crate) probe_write_errors: u64,
    pub(crate) probe_read_errors: u64,
    pub(crate) packets_rx: u64,
    pub(crate) packets_tx: u64,
    pub(crate) frames_rx: u64,
    pub(crate) frames_tx: u64,
    pub(crate) notification_bytes_rx: u64,
    pub(crate) bytes_rx: u64,
    pub(crate) bytes_tx: u64,
    pub(crate) write_chunks_tx: u64,
    pub(crate) serialize_errors: u64,
    pub(crate) hdlc_encode_errors: u64,
    pub(crate) hdlc_decode_errors: u64,
    pub(crate) deserialize_errors: u64,
    pub(crate) rx_queue_errors: u64,
    pub(crate) write_errors: u64,
    pub(crate) read_errors: u64,
    pub(crate) stale_buffer_drops: u64,
    pub(crate) cleanup_errors: u64,
    pub(crate) last_error: Option<String>,
}

impl BleRuntimeStatus {
    #[must_use]
    pub(crate) fn from_settings(settings: &BleRuntimeSettings) -> Self {
        Self {
            link_state: "configured".to_string(),
            adapter: settings.adapter.clone(),
            peripheral_id: settings.peripheral_id.clone(),
            service_uuid: settings.service_uuid.clone(),
            write_char_uuid: settings.write_char_uuid.clone(),
            notify_char_uuid: settings.notify_char_uuid.clone(),
            mtu: settings.mtu,
            scan_timeout_ms: settings.scan_timeout.as_millis() as u64,
            connect_timeout_ms: settings.connect_timeout.as_millis() as u64,
            iface: None,
            connected: false,
            subscribed: false,
            reconnect_attempts: 0,
            scan_errors: 0,
            connect_errors: 0,
            subscribe_errors: 0,
            probe_write_errors: 0,
            probe_read_errors: 0,
            packets_rx: 0,
            packets_tx: 0,
            frames_rx: 0,
            frames_tx: 0,
            notification_bytes_rx: 0,
            bytes_rx: 0,
            bytes_tx: 0,
            write_chunks_tx: 0,
            serialize_errors: 0,
            hdlc_encode_errors: 0,
            hdlc_decode_errors: 0,
            deserialize_errors: 0,
            rx_queue_errors: 0,
            write_errors: 0,
            read_errors: 0,
            stale_buffer_drops: 0,
            cleanup_errors: 0,
            last_error: None,
        }
    }

    #[must_use]
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "link_state": self.link_state,
            "adapter": self.adapter,
            "peripheral_id": self.peripheral_id,
            "service_uuid": self.service_uuid,
            "write_char_uuid": self.write_char_uuid,
            "notify_char_uuid": self.notify_char_uuid,
            "mtu": self.mtu,
            "scan_timeout_ms": self.scan_timeout_ms,
            "connect_timeout_ms": self.connect_timeout_ms,
            "iface": self.iface,
            "connected": self.connected,
            "subscribed": self.subscribed,
            "reconnect_attempts": self.reconnect_attempts,
            "scan_errors": self.scan_errors,
            "connect_errors": self.connect_errors,
            "subscribe_errors": self.subscribe_errors,
            "probe_write_errors": self.probe_write_errors,
            "probe_read_errors": self.probe_read_errors,
            "packets_rx": self.packets_rx,
            "packets_tx": self.packets_tx,
            "frames_rx": self.frames_rx,
            "frames_tx": self.frames_tx,
            "notification_bytes_rx": self.notification_bytes_rx,
            "bytes_rx": self.bytes_rx,
            "bytes_tx": self.bytes_tx,
            "write_chunks_tx": self.write_chunks_tx,
            "serialize_errors": self.serialize_errors,
            "hdlc_encode_errors": self.hdlc_encode_errors,
            "hdlc_decode_errors": self.hdlc_decode_errors,
            "deserialize_errors": self.deserialize_errors,
            "rx_queue_errors": self.rx_queue_errors,
            "write_errors": self.write_errors,
            "read_errors": self.read_errors,
            "stale_buffer_drops": self.stale_buffer_drops,
            "cleanup_errors": self.cleanup_errors,
            "last_error": self.last_error,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BleRuntimeStatusHandle {
    inner: Arc<Mutex<BleRuntimeStatus>>,
}

impl BleRuntimeStatusHandle {
    #[must_use]
    pub(crate) fn new(status: BleRuntimeStatus) -> Self {
        Self { inner: Arc::new(Mutex::new(status)) }
    }

    pub(crate) fn update(&self, update: impl FnOnce(&mut BleRuntimeStatus)) {
        update(&mut self.inner.lock().expect("ble runtime status mutex poisoned"));
    }

    #[must_use]
    pub(crate) fn snapshot(&self) -> BleRuntimeStatus {
        self.inner.lock().expect("ble runtime status mutex poisoned").clone()
    }

    #[must_use]
    pub(crate) fn to_json(&self) -> serde_json::Value {
        self.snapshot().to_json()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BleSpawnResult {
    pub(crate) iface: rns_transport::hash::AddressHash,
    pub(crate) status: BleRuntimeStatusHandle,
}
