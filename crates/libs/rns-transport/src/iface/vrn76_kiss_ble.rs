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

#[cfg(feature = "vrn76-kiss-ble")]
fn parse_vrn76_uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("VR-N76 UUID constants must be valid")
}

#[derive(Debug)]
pub struct Vrn76KissBleRuntime<B> {
    backend: B,
    session: Vrn76KissBleSession,
    connected: bool,
    pending_packets: VecDeque<Vec<u8>>,
    startup_write_failures: usize,
}

impl<B> Vrn76KissBleRuntime<B> {
    #[must_use]
    pub fn new(backend: B, config: Vrn76KissBleConfig) -> Self {
        Self {
            backend,
            session: Vrn76KissBleSession::new(config),
            connected: false,
            pending_packets: VecDeque::new(),
            startup_write_failures: 0,
        }
    }

    #[must_use]
    pub fn backend(&self) -> &B {
        &self.backend
    }

    #[must_use]
    pub fn into_backend(self) -> B {
        self.backend
    }

    #[must_use]
    pub fn status(&self) -> Vrn76KissBleStatus {
        self.session.status_with_connection(
            self.connected,
            self.pending_packets.len(),
            self.startup_write_failures,
        )
    }
}

impl<B> Vrn76KissBleRuntime<B>
where
    B: Vrn76KissBleBackend,
{
    pub async fn connect_and_configure(&mut self) -> Result<(), Vrn76KissBleError> {
        self.connected = false;
        self.reset_session_state();
        self.backend
            .connect()
            .await
            .map_err(|message| Vrn76KissBleError::Backend { operation: "connect", message })?;
        self.backend.subscribe_indications().await.map_err(|message| {
            Vrn76KissBleError::Backend { operation: "subscribe_indications", message }
        })?;
        let writes = self.session.startup_frames();
        self.startup_write_failures = self.write_startup_commands(writes).await;
        self.connected = true;
        Ok(())
    }

    fn reset_session_state(&mut self) {
        let config = self.session.config.clone();
        self.session = Vrn76KissBleSession::new(config);
        self.pending_packets.clear();
        self.startup_write_failures = 0;
    }

    pub async fn send_packet(&mut self, payload: &[u8]) -> Result<(), Vrn76KissBleError> {
        if payload.len() > self.session.config.mtu {
            return Err(Vrn76KissBleError::PacketTooLarge {
                limit: self.session.config.mtu,
                actual: payload.len(),
            });
        }
        let writes = self.session.enqueue_packet(payload);
        self.write_all(writes, "write_packet").await
    }

    pub async fn send_id_beacon(&mut self) -> Result<(), Vrn76KissBleError> {
        let writes = self.session.enqueue_id_beacon();
        self.write_all(writes, "write_id_beacon").await
    }

    pub async fn poll_next_packet(&mut self) -> Result<Option<Vec<u8>>, Vrn76KissBleError> {
        if let Some(packet) = self.pending_packets.pop_front() {
            return Ok(Some(packet));
        }

        let Some(indication) = self.backend.next_indication().await.map_err(|message| {
            self.connected = false;
            Vrn76KissBleError::Backend { operation: "next_indication", message }
        })?
        else {
            return Ok(None);
        };

        let mut packets = self.session.accept_indication(&indication)?;
        let writes = self.session.take_pending_writes();
        self.write_all(writes, "ready_write").await?;
        self.pending_packets.extend(packets.drain(..));
        Ok(self.pending_packets.pop_front())
    }

    async fn write_all(
        &mut self,
        writes: Vec<BleWrite>,
        operation: &'static str,
    ) -> Result<(), Vrn76KissBleError> {
        for write in writes {
            self.backend.write(write).await.map_err(|message| {
                self.connected = false;
                Vrn76KissBleError::Backend { operation, message }
            })?;
        }
        Ok(())
    }

    async fn write_startup_commands(&mut self, writes: Vec<BleWrite>) -> usize {
        let mut failures = 0;
        for write in writes {
            if self.backend.write(write).await.is_err() {
                failures += 1;
            }
        }
        failures
    }
}

#[derive(Debug, Clone)]
pub struct Vrn76KissBleSession {
    config: Vrn76KissBleConfig,
    decoder: KissStreamDecoder,
    last_read_at: StdInstant,
    subscribed: bool,
    interface_ready: bool,
    pending_payloads: VecDeque<Vec<u8>>,
    pending_writes: VecDeque<BleWrite>,
    pending_tnc_fragment: Vec<u8>,
    next_tnc_fragment_id: u8,
    pending_tnc_channel_id: Option<u8>,
}

impl Vrn76KissBleSession {
    #[must_use]
    pub fn new(config: Vrn76KissBleConfig) -> Self {
        Self {
            decoder: KissStreamDecoder::new(config.mtu),
            last_read_at: StdInstant::now(),
            interface_ready: !config.kiss.flow_control,
            subscribed: false,
            pending_payloads: VecDeque::new(),
            pending_writes: VecDeque::new(),
            pending_tnc_fragment: Vec::new(),
            next_tnc_fragment_id: 0,
            pending_tnc_channel_id: None,
            config,
        }
    }

    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    #[must_use]
    pub fn status(&self) -> Vrn76KissBleStatus {
        self.status_with_connection(false, 0, 0)
    }

    fn status_with_connection(
        &self,
        connected: bool,
        pending_packets: usize,
        startup_write_failures: usize,
    ) -> Vrn76KissBleStatus {
        Vrn76KissBleStatus {
            connected,
            subscribed: self.subscribed,
            interface_ready: self.interface_ready,
            startup_write_failures,
            pending_payloads: self.pending_payloads.len(),
            pending_writes: self.pending_writes.len(),
            pending_packets,
        }
    }

    #[must_use]
    pub fn startup_frames(&mut self) -> Vec<BleWrite> {
        self.subscribed = true;
        self.config
            .kiss
            .command_frames()
            .into_iter()
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn enqueue_packet(&mut self, payload: &[u8]) -> Vec<BleWrite> {
        if self.config.kiss.flow_control && !self.interface_ready {
            self.pending_payloads.push_back(payload.to_vec());
            return Vec::new();
        }

        let writes = self.kiss_writes(encode_data_frame(payload));
        if self.config.kiss.flow_control {
            self.interface_ready = false;
        }
        writes
    }

    #[must_use]
    pub fn id_beacon_write(&self) -> Option<BleWrite> {
        self.config.kiss.id_beacon.as_ref().and_then(|beacon| {
            self.kiss_writes(encode_data_frame(&beacon.payload())).into_iter().next()
        })
    }

    #[must_use]
    pub fn enqueue_id_beacon(&mut self) -> Vec<BleWrite> {
        let Some(beacon) = self.config.kiss.id_beacon.as_ref() else {
            return Vec::new();
        };
        let payload = beacon.payload();
        if self.config.kiss.flow_control && !self.interface_ready {
            self.pending_payloads.push_back(payload);
            return Vec::new();
        }

        let writes = self.kiss_writes(encode_data_frame(&payload));
        if self.config.kiss.flow_control {
            self.interface_ready = false;
        }
        writes
    }

    pub fn accept_indication(&mut self, payload: &[u8]) -> Result<Vec<Vec<u8>>, Vrn76KissBleError> {
        let kiss_payload = match self.config.frame_mode {
            Vrn76FrameMode::BenshiTncData => {
                let Some(kiss_payload) = self.accept_benshi_data_rxd_event(payload)? else {
                    return Ok(Vec::new());
                };
                kiss_payload
            }
            Vrn76FrameMode::RawKiss => payload.to_vec(),
        };
        if self.decoder.has_partial_frame()
            && self.last_read_at.elapsed() >= self.config.read_frame_timeout
        {
            self.decoder.clear_partial_frame();
        }
        self.last_read_at = StdInstant::now();
        let frames = self.decoder.push_bytes(&kiss_payload)?;
        let mut packets = Vec::new();
        for frame in frames {
            match frame {
                KissFrame::Data(payload) => {
                    let is_id_beacon = self
                        .config
                        .kiss
                        .id_beacon
                        .as_ref()
                        .is_some_and(|beacon| beacon.matches_payload(&payload));
                    if !is_id_beacon {
                        packets.push(payload);
                    }
                }
                KissFrame::Command(KissCommand::Ready) => {
                    self.interface_ready = true;
                    self.flush_pending_payloads();
                }
                KissFrame::Command(KissCommand::Unknown(_, _)) => {}
            }
        }
        Ok(packets)
    }

    fn accept_benshi_data_rxd_event(
        &mut self,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, Vrn76KissBleError> {
        let fragment = match decode_benshi_data_rxd_event(payload) {
            Ok(fragment) => fragment,
            Err(err) => {
                self.reset_tnc_fragment_state();
                return Err(err);
            }
        };
        if fragment.fragment_id != self.next_tnc_fragment_id {
            let expected_fragment_id = self.next_tnc_fragment_id;
            self.reset_tnc_fragment_state();
            return Err(Vrn76KissBleError::UnexpectedTncFragment {
                expected_fragment_id,
                actual_fragment_id: fragment.fragment_id,
            });
        }

        if fragment.fragment_id == 0 {
            self.pending_tnc_channel_id = fragment.channel_id;
        } else if self.pending_tnc_channel_id != fragment.channel_id {
            let expected_channel_id = self.pending_tnc_channel_id;
            self.reset_tnc_fragment_state();
            return Err(Vrn76KissBleError::UnexpectedTncChannel {
                expected_channel_id,
                actual_channel_id: fragment.channel_id,
            });
        }

        self.pending_tnc_fragment.extend_from_slice(fragment.payload);
        if fragment.is_final {
            let kiss_payload = std::mem::take(&mut self.pending_tnc_fragment);
            self.reset_tnc_fragment_state();
            return Ok(Some(kiss_payload));
        }

        self.next_tnc_fragment_id = self.next_tnc_fragment_id.saturating_add(1);
        Ok(None)
    }

    fn reset_tnc_fragment_state(&mut self) {
        self.pending_tnc_fragment.clear();
        self.next_tnc_fragment_id = 0;
        self.pending_tnc_channel_id = None;
    }

    #[must_use]
    pub fn take_pending_writes(&mut self) -> Vec<BleWrite> {
        self.pending_writes.drain(..).collect()
    }

    fn flush_pending_payloads(&mut self) {
        while self.interface_ready {
            let Some(payload) = self.pending_payloads.pop_front() else {
                break;
            };
            self.pending_writes.extend(self.kiss_writes(encode_data_frame(&payload)));
            if self.config.kiss.flow_control {
                self.interface_ready = false;
            }
        }
    }

    fn kiss_writes(&self, kiss_payload: Vec<u8>) -> Vec<BleWrite> {
        match self.config.frame_mode {
            Vrn76FrameMode::BenshiTncData => self.benshi_writes(&kiss_payload),
            Vrn76FrameMode::RawKiss => {
                self.raw_kiss_write_chunks(&kiss_payload).map(Self::write_with_response).collect()
            }
        }
    }

    fn raw_kiss_write_chunks<'a>(&self, payload: &'a [u8]) -> impl Iterator<Item = Vec<u8>> + 'a {
        let chunk_len = self.config.max_write_len.max(1);
        payload.chunks(chunk_len).map(<[u8]>::to_vec)
    }

    fn benshi_writes(&self, kiss_payload: &[u8]) -> Vec<BleWrite> {
        let fragment_payload_len = self
            .config
            .max_write_len
            .saturating_sub(BENSHI_MESSAGE_HEADER_LEN + TNC_FRAGMENT_HEADER_LEN)
            .max(1);
        let chunk_count = kiss_payload.len().div_ceil(fragment_payload_len).max(1);
        let mut writes = Vec::with_capacity(chunk_count);
        for (index, chunk) in kiss_payload.chunks(fragment_payload_len).enumerate() {
            writes.push(Self::write_with_response(encode_benshi_ht_send_data_fragment(
                index as u8,
                index + 1 == chunk_count,
                chunk,
            )));
        }
        if kiss_payload.is_empty() {
            writes.push(Self::write_with_response(encode_benshi_ht_send_data_fragment(
                0,
                true,
                &[],
            )));
        }
        writes
    }

    fn write_with_response(payload: Vec<u8>) -> BleWrite {
        BleWrite {
            characteristic_uuid: VRN76_WRITE_CHARACTERISTIC_UUID,
            with_response: true,
            payload,
        }
    }
}

#[must_use]
pub fn encode_benshi_ht_send_data(kiss_payload: &[u8]) -> Vec<u8> {
    encode_benshi_ht_send_data_fragment(0, true, kiss_payload)
}

#[must_use]
pub fn encode_benshi_ht_send_data_fragment(
    fragment_id: u8,
    is_final: bool,
    kiss_payload: &[u8],
) -> Vec<u8> {
    let mut frame = encode_benshi_message(false, BENSHI_COMMAND_HT_SEND_DATA);
    frame.extend_from_slice(&encode_tnc_data_fragment(fragment_id, is_final, kiss_payload));
    frame
}

#[cfg(feature = "vrn76-kiss-ble")]
#[derive(Debug, Clone)]
pub struct NativeVrn76KissBleInterface {
    label: String,
    settings: NativeVrn76BleSettings,
    config: Vrn76KissBleConfig,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
}

#[cfg(feature = "vrn76-kiss-ble")]
impl NativeVrn76KissBleInterface {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        settings: NativeVrn76BleSettings,
        config: Vrn76KissBleConfig,
    ) -> Self {
        Self {
            label: label.into(),
            settings,
            config,
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
        }
    }

    #[must_use]
    pub fn with_reconnect_backoff(mut self, reconnect_backoff: Duration) -> Self {
        self.reconnect_backoff = reconnect_backoff;
        if self.max_reconnect_backoff < self.reconnect_backoff {
            self.max_reconnect_backoff = self.reconnect_backoff;
        }
        self
    }

    #[must_use]
    pub fn with_max_reconnect_backoff(mut self, max_reconnect_backoff: Duration) -> Self {
        self.max_reconnect_backoff = max_reconnect_backoff.max(self.reconnect_backoff);
        self
    }

    pub async fn spawn(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (label, settings, config, reconnect_backoff, max_reconnect_backoff) = {
            let guard = context.inner.lock().expect("VR-N76 interface mutex poisoned");
            (
                guard.label.clone(),
                guard.settings.clone(),
                guard.config.clone(),
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
            )
        };
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let backend = NativeVrn76BleBackend::new(settings.clone());
            let mut runtime = Vrn76KissBleRuntime::new(backend, config.clone());
            if let Err(err) = runtime.connect_and_configure().await {
                log::warn!(
                    "VR-N76 KISS-over-BLE session setup failed iface={} addr={} err={:?}",
                    label,
                    iface_address,
                    err
                );
                let mut backend = runtime.into_backend();
                let _ = backend.cleanup().await;
                sleep(active_backoff).await;
                active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
                continue;
            }
            let status = runtime.status();
            if status.startup_write_failures > 0 {
                log::warn!(
                    "VR-N76 KISS-over-BLE startup command write failures iface={} addr={} failures={}",
                    label,
                    iface_address,
                    status.startup_write_failures
                );
            }
            active_backoff = reconnect_backoff;
            log::info!(
                "VR-N76 KISS-over-BLE session established iface={} addr={} peripheral_id={}",
                label,
                iface_address,
                settings.peripheral_id
            );

            let mut tx_buffer = vec![0_u8; config.mtu];
            let mut reconnect_needed = false;
            let mut first_tx_at: Option<Instant> = None;
            while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
                while let Ok(message) = tx_channel.try_recv() {
                    let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                    if message.packet.serialize(&mut output).is_err() {
                        log::warn!("VR-N76 packet serialize failed iface={}", label);
                        continue;
                    }
                    if let Err(err) = runtime.send_packet(output.as_slice()).await {
                        log::warn!("VR-N76 packet write failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                    if first_tx_at.is_none() {
                        first_tx_at = Some(Instant::now());
                    }
                }
                if reconnect_needed {
                    break;
                }

                if let (Some(beacon), Some(first_tx)) =
                    (config.kiss.id_beacon.as_ref(), first_tx_at)
                {
                    if first_tx.elapsed() >= beacon.interval {
                        if let Err(err) = runtime.send_id_beacon().await {
                            log::warn!(
                                "VR-N76 station ID write failed iface={} err={:?}",
                                label,
                                err
                            );
                            reconnect_needed = true;
                            break;
                        }
                        first_tx_at = None;
                    }
                }

                match timeout(Duration::from_millis(100), runtime.poll_next_packet()).await {
                    Ok(Ok(Some(payload))) => {
                        if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&payload)) {
                            let _ = rx_channel
                                .send(RxMessage {
                                    address: iface_address,
                                    packet,
                                    source: IfaceSource::None,
                                })
                                .await;
                        }
                    }
                    Ok(Ok(None)) | Err(_) => {}
                    Ok(Err(err)) => {
                        log::warn!("VR-N76 packet read failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                }
            }

            let mut backend = runtime.into_backend();
            let _ = backend.cleanup().await;
            if context.cancel.is_cancelled() || iface_stop.is_cancelled() {
                break;
            }
            if reconnect_needed {
                sleep(active_backoff).await;
                active_backoff = bounded_backoff_next(active_backoff, max_reconnect_backoff);
            }
        }

        iface_stop.cancel();
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
impl Interface for NativeVrn76KissBleInterface {
    fn mtu() -> usize {
        564
    }

    fn configured_mtu(&self) -> usize {
        self.config.mtu
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

#[must_use]
pub fn encode_benshi_data_rxd_event(kiss_payload: &[u8]) -> Vec<u8> {
    let mut frame = encode_benshi_message(false, BENSHI_COMMAND_EVENT_NOTIFICATION);
    frame.push(BENSHI_EVENT_DATA_RXD);
    frame.extend_from_slice(&encode_tnc_data_fragment(0, true, kiss_payload));
    frame
}

fn encode_benshi_message(is_reply: bool, command: u16) -> Vec<u8> {
    let command_word = (u16::from(is_reply) << 15) | (command & 0x7fff);
    let mut frame = Vec::with_capacity(4);
    frame.extend_from_slice(&BENSHI_COMMAND_GROUP_BASIC.to_be_bytes());
    frame.extend_from_slice(&command_word.to_be_bytes());
    frame
}

fn encode_tnc_data_fragment(fragment_id: u8, is_final: bool, kiss_payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(kiss_payload.len() + 1);
    frame.push((u8::from(is_final) << 7) | (fragment_id & 0x3f));
    frame.extend_from_slice(kiss_payload);
    frame
}

struct TncDataFragment<'a> {
    fragment_id: u8,
    is_final: bool,
    channel_id: Option<u8>,
    payload: &'a [u8],
}

fn decode_benshi_data_rxd_event(payload: &[u8]) -> Result<TncDataFragment<'_>, Vrn76KissBleError> {
    let (command_group, is_reply, command, body) = decode_benshi_message_header(payload)?;
    if command_group != BENSHI_COMMAND_GROUP_BASIC
        || is_reply
        || command != BENSHI_COMMAND_EVENT_NOTIFICATION
    {
        return Err(Vrn76KissBleError::UnsupportedBenshiMessage { command_group, command });
    }
    let Some((&event_type, event_body)) = body.split_first() else {
        return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
    };
    if event_type != BENSHI_EVENT_DATA_RXD {
        return Err(Vrn76KissBleError::UnsupportedBenshiEvent { event_type });
    }
    decode_tnc_data_fragment(event_body)
}

fn decode_benshi_message_header(
    payload: &[u8],
) -> Result<(u16, bool, u16, &[u8]), Vrn76KissBleError> {
    if payload.len() < 4 {
        return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
    }
    let command_group = u16::from_be_bytes([payload[0], payload[1]]);
    let command_word = u16::from_be_bytes([payload[2], payload[3]]);
    Ok((command_group, (command_word & 0x8000) != 0, command_word & 0x7fff, &payload[4..]))
}

fn decode_tnc_data_fragment(payload: &[u8]) -> Result<TncDataFragment<'_>, Vrn76KissBleError> {
    let Some((&header, rest)) = payload.split_first() else {
        return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
    };
    let is_final_fragment = (header & 0x80) != 0;
    let has_channel_id = (header & 0x40) != 0;
    let fragment_id = header & 0x3f;
    let (payload, channel_id) = if has_channel_id {
        let Some((&channel_id, payload)) = rest.split_last() else {
            return Err(Vrn76KissBleError::BenshiFrameTooShort { actual: payload.len() });
        };
        (payload, Some(channel_id))
    } else {
        (rest, None)
    };
    Ok(TncDataFragment { fragment_id, is_final: is_final_fragment, channel_id, payload })
}
