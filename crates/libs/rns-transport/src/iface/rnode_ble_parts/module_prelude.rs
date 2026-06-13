use std::collections::VecDeque;

#[cfg(feature = "rnode-ble")]
use std::pin::Pin;

use std::time::{Duration, Instant};

#[cfg(feature = "rnode-ble")]
use crate::buffer::{InputBuffer, OutputBuffer};

#[cfg(feature = "rnode-ble")]
use crate::iface::{IfaceSource, Interface, InterfaceContext, RxMessage};

#[cfg(feature = "rnode-ble")]
use crate::packet::Packet;

#[cfg(feature = "rnode-ble")]
use crate::serde::Serialize;

#[cfg(feature = "rnode-ble")]
use btleplug::api::{
    Central, CharPropFlags, Characteristic, DEFAULT_MTU_SIZE, Manager as _, Peripheral as _,
    ScanFilter, ValueNotification, WriteType,
};

#[cfg(feature = "rnode-ble")]
use btleplug::platform::{Adapter, Manager, Peripheral};

#[cfg(feature = "rnode-ble")]
use futures::{stream::Stream, StreamExt};

#[cfg(feature = "rnode-ble")]
use tokio::time::{sleep, timeout, Instant as TokioInstant};

#[cfg(feature = "rnode-ble")]
use uuid::Uuid;

use crate::iface::kiss::KissConfig;

use crate::iface::lora::{
    LoraConfig, LoraInterface, RNodeHardwareError, RNodeProbeStatus, RNodeRadioStatus,
};

use crate::kiss::{encode_data_frame, KissCommand, KissDecodeError, KissFrame, KissStreamDecoder};

pub const RNODE_BLE_SERVICE_UUID: &str = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E";

pub const RNODE_BLE_WRITE_CHARACTERISTIC_UUID: &str = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E";

pub const RNODE_BLE_TX_CHARACTERISTIC_UUID: &str = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E";

pub const RNODE_BLE_SCAN_TIMEOUT: Duration = Duration::from_secs(2);

pub const RNODE_BLE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub const RNODE_BLE_READ_FRAME_TIMEOUT: Duration = Duration::from_millis(1_250);

const DEFAULT_ATT_NOTIFICATION_PAYLOAD_BYTES: usize = 20;

#[cfg(feature = "rnode-ble")]
const RNODE_BLE_STARTUP_STABILIZATION_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(feature = "rnode-ble")]
const RNODE_BLE_STARTUP_NOTIFICATION_QUIET_TIMEOUT: Duration = Duration::from_millis(100);

#[cfg(feature = "rnode-ble")]
type NativeNotificationStream = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

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

    async fn next_notification(&mut self) -> Result<Option<Vec<u8>>, String>;

    fn negotiated_mtu(&self) -> Option<u16> {
        None
    }
}

#[cfg(feature = "rnode-ble")]
#[derive(Debug, Clone)]
pub struct NativeRnodeBleSettings {
    pub adapter: Option<String>,
    pub peripheral_id: String,
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
}

#[cfg(feature = "rnode-ble")]
pub struct NativeRnodeBleBackend {
    settings: NativeRnodeBleSettings,
    adapter: Option<Adapter>,
    peripheral: Option<Peripheral>,
    write_char: Option<Characteristic>,
    notify_char: Option<Characteristic>,
    notification_stream: Option<NativeNotificationStream>,
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
            negotiated_mtu: None,
        }
    }

    #[must_use]
    pub fn negotiated_mtu(&self) -> Option<u16> {
        self.negotiated_mtu
    }

    pub async fn cleanup(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        if let (Some(peripheral), Some(notify_char)) =
            (self.peripheral.as_ref(), self.notify_char.as_ref())
        {
            if let Err(err) = peripheral.unsubscribe(notify_char).await {
                failures.push(format!("unsubscribe RNode BLE notify characteristic: {err}"));
            }
        }
        if let Some(adapter) = self.adapter.as_ref() {
            if let Err(err) = adapter.stop_scan().await {
                failures.push(format!("stop BLE scan: {err}"));
            }
        }
        if let Some(peripheral) = self.peripheral.as_ref() {
            match peripheral.is_connected().await {
                Ok(true) => {
                    if let Err(err) = peripheral.disconnect().await {
                        failures.push(format!("disconnect peripheral: {err}"));
                    }
                }
                Ok(false) => {}
                Err(err) => failures.push(format!("read connection state: {err}")),
            }
        }
        self.clear_session_state();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn clear_session_state(&mut self) {
        self.adapter = None;
        self.peripheral = None;
        self.write_char = None;
        self.notify_char = None;
        self.notification_stream = None;
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
    ) -> Result<Peripheral, String> {
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|err| format!("start BLE scan: {err}"))?;
        let deadline = tokio::time::Instant::now() + settings.scan_timeout;
        loop {
            for peripheral in
                adapter.peripherals().await.map_err(|err| format!("list peripherals: {err}"))?
            {
                if rnode_peripheral_matches(&peripheral, &settings.peripheral_id).await? {
                    return Ok(peripheral);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "scan timeout waiting for RNode BLE peripheral_id={}",
                    settings.peripheral_id
                ));
            }
            sleep(Duration::from_millis(200)).await;
        }
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
}

#[cfg(feature = "rnode-ble")]
impl RnodeBleBackend for NativeRnodeBleBackend {
    fn negotiated_mtu(&self) -> Option<u16> {
        self.negotiated_mtu
    }

    async fn connect(&mut self) -> Result<(), String> {
        self.clear_session_state();
        let adapter = Self::select_adapter(&self.settings).await?;
        let peripheral = Self::scan_for_peripheral(&adapter, &self.settings).await?;

        timeout(self.settings.connect_timeout, async {
            let connected = peripheral
                .is_connected()
                .await
                .map_err(|err| format!("read BLE connection state: {err}"))?;
            if !connected {
                peripheral.connect().await.map_err(|err| format!("connect peripheral: {err}"))?;
            }
            peripheral
                .discover_services()
                .await
                .map_err(|err| format!("discover GATT services: {err}"))
        })
        .await
        .map_err(|_| {
            format!("connect timeout after {} ms", self.settings.connect_timeout.as_millis())
        })??;

        self.adapter = Some(adapter);
        self.peripheral = Some(peripheral);
        let mtu = self.peripheral.as_ref().expect("just set above").mtu();
        // On macOS, CoreBluetooth never updates its cached AtomicU16, so peripheral.mtu()
        // always returns DEFAULT_MTU_SIZE (23) regardless of the actual negotiated value.
        // On all other platforms btleplug reports the real negotiated MTU, including 23
        // when that is genuinely what was negotiated.
        self.negotiated_mtu =
            if cfg!(target_os = "macos") && mtu == DEFAULT_MTU_SIZE { None } else { Some(mtu) };
        self.resolve_characteristics()
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
        let notification = timeout(self.settings.notification_timeout, stream.as_mut().next())
            .await
            .map_err(|_| {
                format!(
                    "notification timeout after {} ms",
                    self.settings.notification_timeout.as_millis()
                )
            })?;
        let Some(notification) = notification else {
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
