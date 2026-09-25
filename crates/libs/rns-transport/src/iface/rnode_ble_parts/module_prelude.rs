use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[cfg(feature = "rnode-ble")]
use std::pin::Pin;

use std::time::{Duration, Instant};

#[cfg(feature = "rnode-ble")]
use crate::iface::{
    decode_packet_ifac, encode_packet_ifac, is_ifac_violation, record_ifac_violation, IfaceSource,
    Interface, InterfaceContext, RxMessage,
};

#[cfg(feature = "rnode-ble")]
use btleplug::api::{
    Central, CharPropFlags, Characteristic, DEFAULT_MTU_SIZE, Manager as _, Peripheral as _,
    ScanFilter, ValueNotification, WriteType,
};

#[cfg(all(feature = "rnode-ble", target_os = "android"))]
use btleplug::api::BDAddr;

#[cfg(feature = "rnode-ble")]
use btleplug::platform::{Adapter, Manager, Peripheral};

#[cfg(all(feature = "rnode-ble", target_os = "android"))]
use btleplug::platform::PeripheralId;

#[cfg(feature = "rnode-ble")]
use futures::{stream::Stream, StreamExt};

use tokio::time::{timeout, Instant as TokioInstant};

#[cfg(feature = "rnode-ble")]
use tokio::time::sleep;

#[cfg(feature = "rnode-ble")]
use uuid::Uuid;

use crate::iface::kiss::KissConfig;

use crate::iface::lora::{
    LoraConfig, LoraInterface, RNodeHardwareError, RNodeProbeStatus, RNodeRadioStatus, CMD_STAT_RX,
    CMD_STAT_TX,
};

use crate::kiss::{
    encode_data_frame, KissCommand, KissDecodeError, KissFrame, KissStreamDecoder, CMD_READY,
};

pub const RNODE_BLE_SERVICE_UUID: &str = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E";

pub const RNODE_BLE_WRITE_CHARACTERISTIC_UUID: &str = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E";

pub const RNODE_BLE_TX_CHARACTERISTIC_UUID: &str = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E";

pub const RNODE_BLE_SCAN_TIMEOUT: Duration = Duration::from_secs(2);

pub const RNODE_BLE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub const RNODE_BLE_READ_FRAME_TIMEOUT: Duration = Duration::from_millis(1_250);

/// Minimum delay between RNode firmware queue-admission probes.
///
/// RNode firmware answers `CMD_READY` with its current queue state; it does not
/// emit a later notification when a full queue drains. Polling is therefore
/// required while payloads are pending, but it must remain bounded.
pub const RNODE_QUEUE_ADMISSION_POLL_INTERVAL: Duration = Duration::from_millis(100);

const RNODE_BLE_STARTUP_STABILIZATION_TIMEOUT: Duration = Duration::from_secs(2);

const RNODE_BLE_STARTUP_NOTIFICATION_QUIET_TIMEOUT: Duration = Duration::from_millis(100);

#[cfg(feature = "rnode-ble")]
const RNODE_BLE_MANAGEMENT_CHANNEL_CAPACITY: usize = 64;

#[cfg(feature = "rnode-ble")]
type NativeNotificationStream = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

#[cfg(feature = "rnode-ble")]
type RnodeBleManagementFrameSender = tokio::sync::mpsc::Sender<Vec<u8>>;

#[cfg(feature = "rnode-ble")]
type RnodeBleManagementFrameReceiver =
    Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Vec<u8>>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RnodeBleKissConfig {
    pub service_uuid: &'static str,
    pub write_characteristic_uuid: &'static str,
    pub notify_characteristic_uuid: &'static str,
    pub scan_timeout: Duration,
    pub connect_timeout: Duration,
    pub read_frame_timeout: Duration,
    pub mtu: usize,
    pub max_write_len: usize,
    pub write_with_response: bool,
    pub initial_frames: Vec<Vec<u8>>,
    pub deferred_frames: Vec<Vec<u8>>,
    pub shutdown_frames: Vec<Vec<u8>>,
    pub kiss: KissConfig,
}

impl Default for RnodeBleKissConfig {
    fn default() -> Self {
        Self {
            service_uuid: RNODE_BLE_SERVICE_UUID,
            write_characteristic_uuid: RNODE_BLE_WRITE_CHARACTERISTIC_UUID,
            notify_characteristic_uuid: RNODE_BLE_TX_CHARACTERISTIC_UUID,
            scan_timeout: RNODE_BLE_SCAN_TIMEOUT,
            connect_timeout: RNODE_BLE_CONNECT_TIMEOUT,
            read_frame_timeout: RNODE_BLE_READ_FRAME_TIMEOUT,
            mtu: 508,
            max_write_len: 20,
            write_with_response: false,
            initial_frames: Vec::new(),
            deferred_frames: Vec::new(),
            shutdown_frames: Vec::new(),
            kiss: KissConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RnodeBleWrite {
    pub characteristic_uuid: &'static str,
    pub with_response: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RnodeBleKissStatus {
    pub connected: bool,
    pub subscribed: bool,
    pub interface_ready: bool,
    pub pending_payloads: usize,
    pub pending_writes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RnodeBleKissIoStats {
    pub read_chunks: u64,
    pub read_bytes: u64,
    pub write_chunks: u64,
    pub write_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RnodeBleNotification {
    pub packets: Vec<Vec<u8>>,
    pub commands: Vec<(u8, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RnodeBleKissError {
    Kiss(KissDecodeError),
    Backend { operation: &'static str, message: String },
    PacketTooLarge { limit: usize, actual: usize },
}

impl From<KissDecodeError> for RnodeBleKissError {
    fn from(value: KissDecodeError) -> Self {
        Self::Kiss(value)
    }
}

#[allow(async_fn_in_trait)]
pub trait RnodeBleBackend {
    async fn connect(&mut self) -> Result<(), String>;

    async fn subscribe_notifications(&mut self) -> Result<(), String>;

    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String>;

    /// Read a notification. Native streams use `None` for EOF; compatibility
    /// backends may use it for an idle read (see `notification_stream_ends_on_none`).
    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String>;

    /// Whether `None` is permanent EOF rather than a temporarily empty queue.
    /// The default preserves platform-owned bearer and compatibility backends.
    fn notification_stream_ends_on_none(&self) -> bool {
        false
    }

    /// Whether startup should discard notifications queued before the probe frames.
    ///
    /// Compatibility and platform-owned backends default to preserving all bytes.
    /// Native BLE enables this to discard stale notifications left by an earlier
    /// GATT subscription.
    fn drains_stale_startup_notifications(&self) -> bool {
        false
    }

    /// Close the current connection and release native resources.
    ///
    /// Compatibility backends may rely on this no-op default for one release.
    async fn close(&mut self) -> Result<(), String> {
        Ok(())
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        self.close().await
    }

    fn negotiated_mtu(&self) -> Option<u16> {
        None
    }
}

#[cfg(feature = "rnode-ble")]
#[derive(Debug, Clone)]
pub struct NativeRnodeBleSettings {
    pub adapter: Option<String>,
    pub peripheral_id: String,
    pub peripheral_aliases: Vec<String>,
    pub service_uuid: Uuid,
    pub write_uuid: Uuid,
    pub notify_uuid: Uuid,
    pub scan_timeout: Duration,
    pub connect_timeout: Duration,
    pub notification_timeout: Duration,
}

#[cfg(feature = "rnode-ble")]
impl NativeRnodeBleSettings {
    #[must_use]
    pub fn for_peripheral(peripheral_id: impl Into<String>) -> Self {
        Self {
            adapter: None,
            peripheral_id: peripheral_id.into(),
            peripheral_aliases: Vec::new(),
            service_uuid: parse_rnode_uuid(RNODE_BLE_SERVICE_UUID),
            write_uuid: parse_rnode_uuid(RNODE_BLE_WRITE_CHARACTERISTIC_UUID),
            notify_uuid: parse_rnode_uuid(RNODE_BLE_TX_CHARACTERISTIC_UUID),
            scan_timeout: RNODE_BLE_SCAN_TIMEOUT,
            connect_timeout: RNODE_BLE_CONNECT_TIMEOUT,
            notification_timeout: RNODE_BLE_READ_FRAME_TIMEOUT,
        }
    }

    #[must_use]
    pub fn with_adapter(mut self, adapter: impl Into<String>) -> Self {
        self.adapter = Some(adapter.into());
        self
    }

    #[must_use]
    pub fn with_peripheral_alias(mut self, alias: impl Into<String>) -> Self {
        let alias = alias.into();
        if !alias.trim().is_empty()
            && !self.peripheral_aliases.iter().any(|existing| {
                native_rnode_identifier_matches(existing, &alias)
                    || native_rnode_identifier_matches(&self.peripheral_id, &alias)
            })
        {
            self.peripheral_aliases.push(alias);
        }
        self
    }
}

#[cfg(feature = "rnode-ble")]
pub struct NativeRnodeBleBackend {
    settings: NativeRnodeBleSettings,
    adapter: Option<Adapter>,
    peripheral: Option<Peripheral>,
    write_char: Option<Characteristic>,
    notify_char: Option<Characteristic>,
    notification_stream: Option<NativeNotificationStream>,
    notification_stream_ended: bool,
    negotiated_mtu: Option<u16>,
}

#[cfg(feature = "rnode-ble")]
impl NativeRnodeBleBackend {
    #[must_use]
    pub fn new(settings: NativeRnodeBleSettings) -> Self {
        Self {
            settings,
            adapter: None,
            peripheral: None,
            write_char: None,
            notify_char: None,
            notification_stream: None,
            notification_stream_ended: false,
            negotiated_mtu: None,
        }
    }

    #[must_use]
    pub fn negotiated_mtu(&self) -> Option<u16> {
        self.negotiated_mtu
    }

    fn clear_session_state(&mut self) {
        self.adapter = None;
        self.peripheral = None;
        self.write_char = None;
        self.notify_char = None;
        self.notification_stream = None;
        self.notification_stream_ended = false;
        self.negotiated_mtu = None;
    }

    async fn select_adapter(settings: &NativeRnodeBleSettings) -> Result<Adapter, String> {
        let manager = Manager::new().await.map_err(|err| format!("create BLE manager: {err}"))?;
        let adapters =
            manager.adapters().await.map_err(|err| format!("enumerate BLE adapters: {err}"))?;
        if adapters.is_empty() {
            return Err("no BLE adapters available on host".to_string());
        }

        if let Some(requested) = settings.adapter.as_deref() {
            let requested = requested.trim();
            for adapter in adapters {
                let adapter_info = adapter
                    .adapter_info()
                    .await
                    .map_err(|err| format!("read adapter info: {err}"))?;
                if native_rnode_identifier_matches(requested, &adapter_info) {
                    return Ok(adapter);
                }
            }
            return Err(format!("configured adapter '{requested}' not found"));
        }

        Ok(adapters.into_iter().next().expect("non-empty adapters checked"))
    }

    async fn scan_for_peripheral(
        adapter: &Adapter,
        settings: &NativeRnodeBleSettings,
        exclude_exact_identifier: Option<&str>,
        allow_service_uuid_match: bool,
        excluded_identifiers: &[String],
        paired_addresses: Option<&[String]>,
    ) -> Result<Peripheral, String> {
        adapter
            .start_scan(if allow_service_uuid_match {
                ScanFilter { services: vec![settings.service_uuid] }
            } else {
                ScanFilter::default()
            })
            .await
            .map_err(|err| format!("start BLE scan: {err}"))?;
        let deadline = tokio::time::Instant::now() + settings.scan_timeout;
        loop {
            for peripheral in
                adapter.peripherals().await.map_err(|err| format!("list peripherals: {err}"))?
            {
                if rnode_peripheral_matches(
                    &peripheral,
                    &settings.peripheral_id,
                    &settings.peripheral_aliases,
                    exclude_exact_identifier,
                    settings.service_uuid,
                    allow_service_uuid_match,
                    excluded_identifiers,
                    paired_addresses,
                )
                .await?
                {
                    Self::stop_scan_after_selection(adapter).await;
                    return Ok(peripheral);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                Self::stop_scan_after_selection(adapter).await;
                return Err(format!(
                    "scan timeout waiting for RNode BLE peripheral_id={}",
                    settings.peripheral_id
                ));
            }
            sleep(Duration::from_millis(200)).await;
        }
    }

    async fn stop_scan_after_selection(adapter: &Adapter) {
        if let Err(err) = adapter.stop_scan().await {
            log::debug!("RNode BLE stop scan after selection failed err={err}");
        }
    }

    #[cfg(target_os = "android")]
    async fn configured_peripheral(
        adapter: &Adapter,
        settings: &NativeRnodeBleSettings,
    ) -> Result<Option<Peripheral>, String> {
        let Ok(address) = settings.peripheral_id.parse::<BDAddr>() else {
            return Ok(None);
        };
        let peripheral_id: PeripheralId = match serde_json::to_value(address)
            .and_then(serde_json::from_value)
        {
            Ok(id) => id,
            Err(err) => {
                log::warn!(
                    "RNode BLE could not build Android peripheral id peripheral_id={} err={}",
                    settings.peripheral_id,
                    err
                );
                return Ok(None);
            }
        };
        match adapter.add_peripheral(&peripheral_id).await {
            Ok(peripheral) => {
                log::info!(
                    "RNode BLE using configured Android paired peripheral_id={}",
                    settings.peripheral_id
                );
                Ok(Some(peripheral))
            }
            Err(err) => {
                log::warn!(
                    "RNode BLE configured Android peripheral unavailable peripheral_id={} err={}",
                    settings.peripheral_id,
                    err
                );
                Ok(None)
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    async fn configured_peripheral(
        _adapter: &Adapter,
        _settings: &NativeRnodeBleSettings,
    ) -> Result<Option<Peripheral>, String> {
        Ok(None)
    }

    fn resolve_characteristics(&mut self) -> Result<(), String> {
        let peripheral =
            self.peripheral.as_ref().ok_or_else(|| "no connected peripheral".to_string())?;
        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|characteristic| {
                characteristic.uuid == self.settings.write_uuid
                    && characteristic.service_uuid == self.settings.service_uuid
            })
            .cloned()
            .ok_or_else(|| {
                format!("RNode BLE write characteristic {} not found", self.settings.write_uuid)
            })?;
        let notify_char = characteristics
            .iter()
            .find(|characteristic| {
                characteristic.uuid == self.settings.notify_uuid
                    && characteristic.service_uuid == self.settings.service_uuid
            })
            .cloned()
            .ok_or_else(|| {
                format!("RNode BLE notify characteristic {} not found", self.settings.notify_uuid)
            })?;

        if !write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
            && !write_char.properties.contains(CharPropFlags::WRITE)
        {
            return Err("RNode BLE write characteristic does not support BLE writes".to_string());
        }
        if !notify_char.properties.contains(CharPropFlags::NOTIFY)
            && !notify_char.properties.contains(CharPropFlags::INDICATE)
        {
            return Err("RNode BLE TX characteristic does not support notifications".to_string());
        }

        self.write_char = Some(write_char);
        self.notify_char = Some(notify_char);
        Ok(())
    }

    async fn connect_selected_peripheral(
        peripheral: &Peripheral,
        connect_timeout: Duration,
    ) -> Result<(), String> {
        timeout(connect_timeout, async {
            let connected = peripheral
                .is_connected()
                .await
                .map_err(|err| format!("read BLE connection state: {err}"))?;
            if !connected {
                peripheral
                    .connect()
                    .await
                    .map_err(|err| format!("connect peripheral: {err}"))?;
            }
            peripheral
                .discover_services()
                .await
                .map_err(|err| format!("discover GATT services: {err}"))
        })
        .await
        .map_err(|_| format!("connect timeout after {} ms", connect_timeout.as_millis()))?
    }
}

#[cfg(feature = "rnode-ble")]
impl RnodeBleBackend for NativeRnodeBleBackend {
    fn negotiated_mtu(&self) -> Option<u16> {
        self.negotiated_mtu
    }

    async fn connect(&mut self) -> Result<(), String> {
        self.cleanup().await?;
        let result = self.connect_session().await;
        if result.is_err() {
            if let Err(error) = self.cleanup().await {
                log::warn!("RNode BLE native setup cleanup failed peripheral_id={} error={error}", self.settings.peripheral_id);
            }
        }
        result
    }

    async fn subscribe_notifications(&mut self) -> Result<(), String> {
        let peripheral =
            self.peripheral.as_ref().ok_or_else(|| "no connected peripheral".to_string())?;
        let notify_char = self
            .notify_char
            .clone()
            .ok_or_else(|| "notify characteristic not resolved".to_string())?;
        let stream =
            peripheral.notifications().await.map_err(|err| format!("open notifications: {err}"))?;
        self.notification_stream = Some(Box::pin(stream));
        peripheral
            .subscribe(&notify_char)
            .await
            .map_err(|err| format!("subscribe RNode BLE notify characteristic: {err}"))
    }

    async fn write(&mut self, write: RnodeBleWrite) -> Result<(), String> {
        if write.characteristic_uuid != RNODE_BLE_WRITE_CHARACTERISTIC_UUID {
            return Err(format!(
                "unexpected RNode BLE write characteristic {}",
                write.characteristic_uuid
            ));
        }
        let peripheral =
            self.peripheral.as_ref().ok_or_else(|| "no connected peripheral".to_string())?;
        let write_char = self
            .write_char
            .clone()
            .ok_or_else(|| "write characteristic not resolved".to_string())?;
        let write_type =
            if write.with_response { WriteType::WithResponse } else { WriteType::WithoutResponse };
        peripheral
            .write(&write_char, &write.payload, write_type)
            .await
            .map_err(|err| format!("write RNode BLE payload: {err}"))
    }

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String> {
        let notify_uuid = self.settings.notify_uuid;
        let stream = self
            .notification_stream
            .as_mut()
            .ok_or_else(|| "notification stream not initialized".to_string())?;
        let notification = match timeout(self.settings.notification_timeout, stream.as_mut().next())
            .await
        {
            Ok(notification) => notification,
            Err(_) => return Ok(None),
        };
        let Some(notification) = notification else {
            self.notification_stream_ended = true;
            return Ok(None);
        };
        if notification.uuid != notify_uuid {
            return Err(format!(
                "notification for unexpected RNode BLE characteristic {}",
                notification.uuid
            ));
        }
        Ok(Some(notification.value))
    }

    fn notification_stream_ends_on_none(&self) -> bool {
        self.notification_stream_ended
    }

    fn drains_stale_startup_notifications(&self) -> bool {
        true
    }

    async fn close(&mut self) -> Result<(), String> {
        self.cleanup().await
    }

    async fn cleanup(&mut self) -> Result<(), String> {
        NativeRnodeBleBackend::cleanup(self).await
    }
}

#[cfg(feature = "rnode-ble")]
pub fn native_rnode_identifier_matches(configured: &str, discovered: &str) -> bool {
    normalize_rnode_identifier(configured) == normalize_rnode_identifier(discovered)
}

#[cfg(feature = "rnode-ble")]
fn normalize_rnode_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}
