use super::fragment_packet;

use reticulum_daemon::config::InterfaceConfig;

use rns_transport::buffer::OutputBuffer;
use rns_transport::hash::AddressHash;
use rns_transport::iface::{Interface, InterfaceContext, InterfaceManager};
use rns_transport::serde::Serialize;

use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) const SERVICE_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e3";
pub(crate) const TX_CHAR_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e4";
pub(crate) const RX_CHAR_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e5";
pub(crate) const IDENTITY_CHAR_UUID: &str = "37145b00-442d-4a94-917f-8f42c5da28e6";

const DEFAULT_MTU: usize = 185;
const MIN_MTU: usize = 23;
const MAX_MTU: usize = 517;
const RETICULUM_BLE_PACKET_BUFFER: usize = 8192;

#[derive(Debug, Clone)]
pub(crate) struct ReticulumBleSpawnResult {
    pub(crate) iface: AddressHash,
    pub(crate) status: ReticulumBleRuntimeStatusHandle,
}

#[derive(Debug, Clone)]
pub(crate) struct ReticulumBleSettings {
    adapter: Option<String>,
    service_uuid: String,
    tx_char_uuid: String,
    rx_char_uuid: String,
    identity_char_uuid: String,
    mtu: usize,
    max_connections: usize,
    scan_duration: Duration,
    discovery_interval: Duration,
    discovery_interval_idle: Duration,
    advertising_refresh_interval: Duration,
    min_rssi_dbm: i32,
    enable_central: bool,
    enable_peripheral: bool,
    local_identity: [u8; 16],
}

impl ReticulumBleSettings {
    pub(crate) fn from_config(
        iface: &InterfaceConfig,
        local_identity: [u8; 16],
    ) -> Result<Self, String> {
        let mtu = iface.mtu.unwrap_or(DEFAULT_MTU);
        if !(MIN_MTU..=MAX_MTU).contains(&mtu) {
            return Err("reticulum_ble.mtu must be between 23 and 517".to_string());
        }
        let enable_central = iface.enable_central.unwrap_or(true);
        let enable_peripheral = iface.enable_peripheral.unwrap_or(true);
        if !enable_central && !enable_peripheral {
            return Err("reticulum_ble.enable_central and enable_peripheral cannot both be false"
                .to_string());
        }
        Ok(Self {
            adapter: iface.adapter.clone(),
            service_uuid: iface.service_uuid.clone().unwrap_or_else(|| SERVICE_UUID.to_string()),
            tx_char_uuid: iface
                .notify_char_uuid
                .clone()
                .unwrap_or_else(|| TX_CHAR_UUID.to_string()),
            rx_char_uuid: iface.write_char_uuid.clone().unwrap_or_else(|| RX_CHAR_UUID.to_string()),
            identity_char_uuid: iface
                .identity_char_uuid
                .clone()
                .unwrap_or_else(|| IDENTITY_CHAR_UUID.to_string()),
            mtu,
            max_connections: iface.max_connections.unwrap_or(7),
            scan_duration: Duration::from_millis(iface.scan_duration_ms.unwrap_or(10_000)),
            discovery_interval: Duration::from_millis(iface.discovery_interval_ms.unwrap_or(5_000)),
            discovery_interval_idle: Duration::from_millis(
                iface.discovery_interval_idle_ms.unwrap_or(30_000),
            ),
            advertising_refresh_interval: Duration::from_millis(
                iface.advertising_refresh_interval_ms.unwrap_or(30_000),
            ),
            min_rssi_dbm: iface.min_rssi_dbm.unwrap_or(-85),
            enable_central,
            enable_peripheral,
            local_identity,
        })
    }
}

#[derive(Debug, Clone)]
struct ReticulumBleRuntimeStatus {
    link_state: String,
    adapter: Option<String>,
    service_uuid: String,
    tx_char_uuid: String,
    rx_char_uuid: String,
    identity_char_uuid: String,
    local_identity: String,
    mtu: usize,
    max_connections: usize,
    scan_duration_ms: u64,
    discovery_interval_ms: u64,
    discovery_interval_idle_ms: u64,
    advertising_refresh_interval_ms: u64,
    min_rssi_dbm: i32,
    enable_central: bool,
    enable_peripheral: bool,
    central_links: u64,
    peripheral_links: u64,
    scan_state: String,
    advertising_state: String,
    peer_identities: Vec<String>,
    active_addresses: Vec<String>,
    fragments_rx: u64,
    fragments_tx: u64,
    packets_rx: u64,
    packets_tx: u64,
    malformed_fragments: u64,
    reconnects: u64,
    duplicate_rejections: u64,
    stale_reassembly_drops: u64,
    serialize_errors: u64,
    last_error: Option<String>,
    iface: Option<String>,
}

impl ReticulumBleRuntimeStatus {
    fn from_settings(settings: &ReticulumBleSettings) -> Self {
        Self {
            link_state: "configured".to_string(),
            adapter: settings.adapter.clone(),
            service_uuid: settings.service_uuid.clone(),
            tx_char_uuid: settings.tx_char_uuid.clone(),
            rx_char_uuid: settings.rx_char_uuid.clone(),
            identity_char_uuid: settings.identity_char_uuid.clone(),
            local_identity: identity_hex(&settings.local_identity),
            mtu: settings.mtu,
            max_connections: settings.max_connections,
            scan_duration_ms: settings.scan_duration.as_millis() as u64,
            discovery_interval_ms: settings.discovery_interval.as_millis() as u64,
            discovery_interval_idle_ms: settings.discovery_interval_idle.as_millis() as u64,
            advertising_refresh_interval_ms: settings.advertising_refresh_interval.as_millis()
                as u64,
            min_rssi_dbm: settings.min_rssi_dbm,
            enable_central: settings.enable_central,
            enable_peripheral: settings.enable_peripheral,
            central_links: 0,
            peripheral_links: 0,
            scan_state: "configured".to_string(),
            advertising_state: "configured".to_string(),
            peer_identities: Vec::new(),
            active_addresses: Vec::new(),
            fragments_rx: 0,
            fragments_tx: 0,
            packets_rx: 0,
            packets_tx: 0,
            malformed_fragments: 0,
            reconnects: 0,
            duplicate_rejections: 0,
            stale_reassembly_drops: 0,
            serialize_errors: 0,
            last_error: None,
            iface: None,
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "link_state": self.link_state,
            "adapter": self.adapter,
            "service_uuid": self.service_uuid,
            "tx_char_uuid": self.tx_char_uuid,
            "rx_char_uuid": self.rx_char_uuid,
            "identity_char_uuid": self.identity_char_uuid,
            "local_identity": self.local_identity,
            "mtu": self.mtu,
            "max_connections": self.max_connections,
            "scan_duration_ms": self.scan_duration_ms,
            "discovery_interval_ms": self.discovery_interval_ms,
            "discovery_interval_idle_ms": self.discovery_interval_idle_ms,
            "advertising_refresh_interval_ms": self.advertising_refresh_interval_ms,
            "min_rssi_dbm": self.min_rssi_dbm,
            "enable_central": self.enable_central,
            "enable_peripheral": self.enable_peripheral,
            "central_links": self.central_links,
            "peripheral_links": self.peripheral_links,
            "scan_state": self.scan_state,
            "advertising_state": self.advertising_state,
            "peer_identities": self.peer_identities,
            "active_addresses": self.active_addresses,
            "fragments_rx": self.fragments_rx,
            "fragments_tx": self.fragments_tx,
            "packets_rx": self.packets_rx,
            "packets_tx": self.packets_tx,
            "malformed_fragments": self.malformed_fragments,
            "reconnects": self.reconnects,
            "duplicate_rejections": self.duplicate_rejections,
            "stale_reassembly_drops": self.stale_reassembly_drops,
            "serialize_errors": self.serialize_errors,
            "last_error": self.last_error,
            "iface": self.iface,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReticulumBleRuntimeStatusHandle {
    inner: Arc<Mutex<ReticulumBleRuntimeStatus>>,
}

impl ReticulumBleRuntimeStatusHandle {
    fn new(status: ReticulumBleRuntimeStatus) -> Self {
        Self { inner: Arc::new(Mutex::new(status)) }
    }

    fn update(&self, f: impl FnOnce(&mut ReticulumBleRuntimeStatus)) {
        if let Ok(mut guard) = self.inner.lock() {
            f(&mut guard);
        }
    }

    pub(crate) fn to_json(&self) -> serde_json::Value {
        self.inner.lock().expect("reticulum BLE runtime status mutex poisoned").to_json()
    }
}

pub(crate) async fn spawn(
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    iface: &InterfaceConfig,
    local_identity: [u8; 16],
) -> Result<ReticulumBleSpawnResult, String> {
    let settings = ReticulumBleSettings::from_config(iface, local_identity)?;
    let status =
        ReticulumBleRuntimeStatusHandle::new(ReticulumBleRuntimeStatus::from_settings(&settings));
    let interface = ReticulumBleInterface {
        settings,
        status: status.clone(),
        label: iface.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
    };
    let iface = iface_manager
        .lock()
        .await
        .spawn(interface, |context| async move { ReticulumBleInterface::run(context).await });
    status.update(|status| status.iface = Some(iface.to_string()));
    Ok(ReticulumBleSpawnResult { iface, status })
}

struct ReticulumBleInterface {
    settings: ReticulumBleSettings,
    status: ReticulumBleRuntimeStatusHandle,
    label: String,
}

impl ReticulumBleInterface {
    async fn run(context: InterfaceContext<Self>) {
        let (settings, status, label) = {
            let guard = context.inner.lock().expect("reticulum BLE interface mutex poisoned");
            (guard.settings.clone(), guard.status.clone(), guard.label.clone())
        };
        status.update(|runtime| {
            runtime.link_state = "degraded".to_string();
            runtime.scan_state = if settings.enable_central {
                "native_backend_pending".to_string()
            } else {
                "disabled".to_string()
            };
            runtime.advertising_state = if settings.enable_peripheral {
                "native_backend_pending".to_string()
            } else {
                "disabled".to_string()
            };
            runtime.last_error =
                Some("reticulum_ble native dual-role backend is not yet available".to_string());
        });
        log::warn!(
            "[daemon] reticulum_ble name={} configured without native dual-role backend",
            label
        );

        let (_, mut tx_channel) = context.channel.split();
        let mut tx_buffer = [0_u8; RETICULUM_BLE_PACKET_BUFFER];
        loop {
            tokio::select! {
                _ = context.cancel.cancelled() => break,
                maybe = tx_channel.recv() => {
                    let Some(message) = maybe else { break };
                    let mut output = OutputBuffer::new(&mut tx_buffer);
                    if message.packet.serialize(&mut output).is_err() {
                        status.update(|runtime| {
                            runtime.serialize_errors = runtime.serialize_errors.saturating_add(1);
                            runtime.last_error = Some("packet serialize failed".to_string());
                        });
                        continue;
                    }
                    match fragment_packet(output.as_slice(), settings.mtu) {
                        Ok(fragments) => status.update(|runtime| {
                            runtime.packets_tx = runtime.packets_tx.saturating_add(1);
                            runtime.fragments_tx = runtime
                                .fragments_tx
                                .saturating_add(fragments.len() as u64);
                        }),
                        Err(err) => status.update(|runtime| {
                            runtime.malformed_fragments = runtime.malformed_fragments.saturating_add(1);
                            runtime.last_error = Some(err.to_string());
                        }),
                    }
                }
            }
        }
        status.update(|runtime| runtime.link_state = "stopped".to_string());
    }
}

impl Interface for ReticulumBleInterface {
    fn mtu() -> usize {
        MAX_MTU
    }

    fn configured_mtu(&self) -> usize {
        self.settings.mtu
    }
}

fn identity_hex(identity: &[u8; 16]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}
