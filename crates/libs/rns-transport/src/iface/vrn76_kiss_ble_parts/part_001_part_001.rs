use std::collections::VecDeque;

use std::time::{Duration, Instant as StdInstant};

#[cfg(feature = "vrn76-kiss-ble")]
use crate::buffer::{InputBuffer, OutputBuffer};

#[cfg(feature = "vrn76-kiss-ble")]
use crate::iface::{IfaceSource, Interface, InterfaceContext, RxMessage};

#[cfg(feature = "vrn76-kiss-ble")]
use crate::packet::Packet;

#[cfg(feature = "vrn76-kiss-ble")]
use crate::serde::Serialize;

#[cfg(feature = "vrn76-kiss-ble")]
use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
};

#[cfg(feature = "vrn76-kiss-ble")]
use btleplug::platform::{Adapter, Manager, Peripheral};

#[cfg(feature = "vrn76-kiss-ble")]
use futures::{stream::Stream, StreamExt};

#[cfg(feature = "vrn76-kiss-ble")]
use std::pin::Pin;

#[cfg(feature = "vrn76-kiss-ble")]
use tokio::time::{sleep, timeout, Instant};

#[cfg(feature = "vrn76-kiss-ble")]
use uuid::Uuid;

use crate::iface::kiss::KissConfig;

use crate::kiss::{encode_data_frame, KissCommand, KissDecodeError, KissFrame, KissStreamDecoder};

pub const VRN76_SERVICE_UUID: &str = "00001100-d102-11e1-9b23-00025b00a5a5";

pub const VRN76_WRITE_CHARACTERISTIC_UUID: &str = "00001101-d102-11e1-9b23-00025b00a5a5";

pub const VRN76_INDICATE_CHARACTERISTIC_UUID: &str = "00001102-d102-11e1-9b23-00025b00a5a5";

pub const VRN76_KISS_READ_FRAME_TIMEOUT: Duration = Duration::from_millis(1_250);

#[cfg(feature = "vrn76-kiss-ble")]
type NativeNotificationStream = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

const BENSHI_COMMAND_GROUP_BASIC: u16 = 2;

const BENSHI_COMMAND_EVENT_NOTIFICATION: u16 = 9;

const BENSHI_COMMAND_HT_SEND_DATA: u16 = 31;

const BENSHI_EVENT_DATA_RXD: u8 = 2;

const BENSHI_MESSAGE_HEADER_LEN: usize = 4;

const TNC_FRAGMENT_HEADER_LEN: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vrn76FrameMode {
    BenshiTncData,
    RawKiss,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vrn76KissBleConfig {
    pub mtu: usize,
    pub max_write_len: usize,
    pub scan_timeout: Duration,
    pub command_timeout: Duration,
    pub read_frame_timeout: Duration,
    pub frame_mode: Vrn76FrameMode,
    pub kiss: KissConfig,
}

impl Default for Vrn76KissBleConfig {
    fn default() -> Self {
        Self {
            mtu: 564,
            max_write_len: 512,
            scan_timeout: Duration::from_millis(10_000),
            command_timeout: Duration::from_millis(3_000),
            read_frame_timeout: VRN76_KISS_READ_FRAME_TIMEOUT,
            frame_mode: Vrn76FrameMode::BenshiTncData,
            kiss: KissConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vrn76KissBleError {
    Kiss(KissDecodeError),
    Backend { operation: &'static str, message: String },
    PacketTooLarge { limit: usize, actual: usize },
    BenshiFrameTooShort { actual: usize },
    UnsupportedBenshiMessage { command_group: u16, command: u16 },
    UnsupportedBenshiEvent { event_type: u8 },
    UnsupportedTncFragment { fragment_id: u8, has_channel_id: bool },
    UnexpectedTncFragment { expected_fragment_id: u8, actual_fragment_id: u8 },
    UnexpectedTncChannel { expected_channel_id: Option<u8>, actual_channel_id: Option<u8> },
}

impl From<KissDecodeError> for Vrn76KissBleError {
    fn from(value: KissDecodeError) -> Self {
        Self::Kiss(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BleWrite {
    pub characteristic_uuid: &'static str,
    pub with_response: bool,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vrn76KissBleStatus {
    pub connected: bool,
    pub subscribed: bool,
    pub interface_ready: bool,
    pub startup_write_failures: usize,
    pub pending_payloads: usize,
    pub pending_writes: usize,
    pub pending_packets: usize,
}

#[allow(async_fn_in_trait)]
pub trait Vrn76KissBleBackend {
    async fn connect(&mut self) -> Result<(), String>;

    async fn subscribe_indications(&mut self) -> Result<(), String>;

    async fn write(&mut self, write: BleWrite) -> Result<(), String>;

    async fn next_indication(&mut self) -> Result<Option<Vec<u8>>, String>;
}

#[cfg(feature = "vrn76-kiss-ble")]
#[derive(Debug, Clone)]
pub struct NativeVrn76BleSettings {
    pub adapter: Option<String>,
    pub peripheral_id: String,
    pub service_uuid: Uuid,
    pub write_uuid: Uuid,
    pub indicate_uuid: Uuid,
    pub scan_timeout: Duration,
    pub connect_timeout: Duration,
    pub notification_timeout: Duration,
}

#[cfg(feature = "vrn76-kiss-ble")]
impl NativeVrn76BleSettings {
    #[must_use]
    pub fn for_peripheral(peripheral_id: impl Into<String>) -> Self {
        Self {
            adapter: None,
            peripheral_id: peripheral_id.into(),
            service_uuid: parse_vrn76_uuid(VRN76_SERVICE_UUID),
            write_uuid: parse_vrn76_uuid(VRN76_WRITE_CHARACTERISTIC_UUID),
            indicate_uuid: parse_vrn76_uuid(VRN76_INDICATE_CHARACTERISTIC_UUID),
            scan_timeout: Duration::from_millis(10_000),
            connect_timeout: Duration::from_millis(3_000),
            notification_timeout: Duration::from_millis(3_000),
        }
    }

    #[must_use]
    pub fn with_adapter(mut self, adapter: impl Into<String>) -> Self {
        self.adapter = Some(adapter.into());
        self
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
pub struct NativeVrn76BleBackend {
    settings: NativeVrn76BleSettings,
    adapter: Option<Adapter>,
    peripheral: Option<Peripheral>,
    write_char: Option<Characteristic>,
    indicate_char: Option<Characteristic>,
    notification_stream: Option<NativeNotificationStream>,
}

#[cfg(feature = "vrn76-kiss-ble")]
impl NativeVrn76BleBackend {
    #[must_use]
    pub fn new(settings: NativeVrn76BleSettings) -> Self {
        Self {
            settings,
            adapter: None,
            peripheral: None,
            write_char: None,
            indicate_char: None,
            notification_stream: None,
        }
    }

    pub async fn cleanup(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();
        if let (Some(peripheral), Some(indicate_char)) =
            (self.peripheral.as_ref(), self.indicate_char.as_ref())
        {
            if let Err(err) = peripheral.unsubscribe(indicate_char).await {
                failures.push(format!("unsubscribe indication characteristic: {err}"));
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
        self.indicate_char = None;
        self.notification_stream = None;
    }

    async fn select_adapter(settings: &NativeVrn76BleSettings) -> Result<Adapter, String> {
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
                if native_vrn76_identifier_matches(requested, &adapter_info) {
                    return Ok(adapter);
                }
            }
            return Err(format!("configured adapter '{requested}' not found"));
        }

        Ok(adapters.into_iter().next().expect("non-empty adapters checked"))
    }

    async fn scan_for_peripheral(
        adapter: &Adapter,
        settings: &NativeVrn76BleSettings,
    ) -> Result<Peripheral, String> {
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|err| format!("start BLE scan: {err}"))?;
        let deadline = Instant::now() + settings.scan_timeout;
        loop {
            for peripheral in
                adapter.peripherals().await.map_err(|err| format!("list peripherals: {err}"))?
            {
                if peripheral_matches(&peripheral, &settings.peripheral_id).await? {
                    return Ok(peripheral);
                }
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "scan timeout waiting for peripheral_id={}",
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
                format!("VR-N76 write characteristic {} not found", self.settings.write_uuid)
            })?;
        let indicate_char = characteristics
            .iter()
            .find(|characteristic| {
                characteristic.uuid == self.settings.indicate_uuid
                    && characteristic.service_uuid == self.settings.service_uuid
            })
            .cloned()
            .ok_or_else(|| {
                format!(
                    "VR-N76 indication characteristic {} not found",
                    self.settings.indicate_uuid
                )
            })?;

        if !write_char.properties.contains(CharPropFlags::WRITE) {
            return Err(
                "VR-N76 write characteristic does not support write-with-response".to_string()
            );
        }
        if !indicate_char.properties.contains(CharPropFlags::INDICATE)
            && !indicate_char.properties.contains(CharPropFlags::NOTIFY)
        {
            return Err("VR-N76 indication characteristic does not support indications".to_string());
        }

        self.write_char = Some(write_char);
        self.indicate_char = Some(indicate_char);
        Ok(())
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
impl Vrn76KissBleBackend for NativeVrn76BleBackend {
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
        self.resolve_characteristics()
    }

    async fn subscribe_indications(&mut self) -> Result<(), String> {
        let peripheral =
            self.peripheral.as_ref().ok_or_else(|| "no connected peripheral".to_string())?;
        let indicate_char = self
            .indicate_char
            .clone()
            .ok_or_else(|| "indication characteristic not resolved".to_string())?;
        let stream =
            peripheral.notifications().await.map_err(|err| format!("open notifications: {err}"))?;
        self.notification_stream = Some(Box::pin(stream));
        peripheral
            .subscribe(&indicate_char)
            .await
            .map_err(|err| format!("subscribe indication characteristic: {err}"))
    }

    async fn write(&mut self, write: BleWrite) -> Result<(), String> {
        if write.characteristic_uuid != VRN76_WRITE_CHARACTERISTIC_UUID {
            return Err(format!("unexpected write characteristic {}", write.characteristic_uuid));
        }
        if !write.with_response {
            return Err("VR-N76 BLE writes must use write-with-response".to_string());
        }
        let peripheral =
            self.peripheral.as_ref().ok_or_else(|| "no connected peripheral".to_string())?;
        let write_char = self
            .write_char
            .clone()
            .ok_or_else(|| "write characteristic not resolved".to_string())?;
        peripheral
            .write(&write_char, &write.payload, WriteType::WithResponse)
            .await
            .map_err(|err| format!("write VR-N76 payload: {err}"))
    }

    async fn next_indication(&mut self) -> Result<Option<Vec<u8>>, String> {
        let indicate_uuid = self.settings.indicate_uuid;
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
        if notification.uuid != indicate_uuid {
            return Err(format!(
                "notification for unexpected characteristic {}",
                notification.uuid
            ));
        }
        Ok(Some(notification.value))
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
pub fn native_vrn76_identifier_matches(configured: &str, discovered: &str) -> bool {
    normalize_vrn76_identifier(configured) == normalize_vrn76_identifier(discovered)
}

#[cfg(feature = "vrn76-kiss-ble")]
fn normalize_vrn76_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(feature = "vrn76-kiss-ble")]
async fn peripheral_matches(peripheral: &Peripheral, configured_id: &str) -> Result<bool, String> {
    if native_vrn76_identifier_matches(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        if native_vrn76_identifier_matches(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if native_vrn76_identifier_matches(configured_id, &local_name) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
