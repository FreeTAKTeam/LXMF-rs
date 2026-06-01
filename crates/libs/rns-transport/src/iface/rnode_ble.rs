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
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
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
        }
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

#[cfg(feature = "rnode-ble")]
async fn rnode_peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
) -> Result<bool, String> {
    if native_rnode_identifier_matches(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    let properties = peripheral
        .properties()
        .await
        .map_err(|err| format!("read peripheral properties: {err}"))?;
    if let Some(properties) = properties {
        if native_rnode_identifier_matches(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if native_rnode_identifier_matches(configured_id, &local_name) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(feature = "rnode-ble")]
fn parse_rnode_uuid(value: &str) -> Uuid {
    Uuid::parse_str(value).expect("RNode BLE UUID constants must be valid")
}

pub struct RnodeBleKissRuntime<B> {
    backend: B,
    session: RnodeBleKissSession,
    connected: bool,
}

impl<B> RnodeBleKissRuntime<B>
where
    B: RnodeBleBackend,
{
    #[must_use]
    pub fn new(backend: B, config: RnodeBleKissConfig) -> Self {
        Self { backend, session: RnodeBleKissSession::new(config), connected: false }
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
    pub fn status(&self) -> RnodeBleKissStatus {
        self.session.status_with_connection(self.connected)
    }

    pub async fn startup(&mut self) -> Result<(), RnodeBleKissError> {
        self.connected = false;
        self.backend
            .connect()
            .await
            .map_err(|message| RnodeBleKissError::Backend { operation: "connect", message })?;
        self.backend.subscribe_notifications().await.map_err(|message| {
            RnodeBleKissError::Backend { operation: "subscribe_notifications", message }
        })?;
        let writes = self.session.startup_frames();
        self.write_all(writes, "startup_write").await?;
        self.connected = true;
        Ok(())
    }

    pub async fn send_packet(&mut self, payload: &[u8]) -> Result<(), RnodeBleKissError> {
        if payload.len() > self.session.mtu() {
            return Err(RnodeBleKissError::PacketTooLarge {
                limit: self.session.mtu(),
                actual: payload.len(),
            });
        }
        let writes = self.session.enqueue_packet(payload);
        self.write_all(writes, "write_packet").await
    }

    pub async fn send_id_beacon(&mut self) -> Result<(), RnodeBleKissError> {
        let writes = self.session.enqueue_id_beacon();
        self.write_all(writes, "write_id_beacon").await
    }

    pub async fn shutdown(&mut self) -> Result<(), RnodeBleKissError> {
        let writes = self.session.shutdown_frames();
        self.write_all(writes, "shutdown_write").await
    }

    pub async fn poll_notification(&mut self) -> Result<Vec<Vec<u8>>, RnodeBleKissError> {
        Ok(self.poll_notification_events().await?.packets)
    }

    pub async fn poll_notification_events(
        &mut self,
    ) -> Result<RnodeBleNotification, RnodeBleKissError> {
        let Some(payload) = self.backend.next_notification().await.map_err(|message| {
            self.connected = false;
            RnodeBleKissError::Backend { operation: "next_notification", message }
        })?
        else {
            return Ok(RnodeBleNotification::default());
        };
        let notification = self.session.accept_notification_events(&payload)?;
        let writes = self.session.take_pending_writes();
        self.write_all(writes, "write_pending").await?;
        Ok(notification)
    }

    async fn write_all(
        &mut self,
        writes: Vec<RnodeBleWrite>,
        operation: &'static str,
    ) -> Result<(), RnodeBleKissError> {
        for write in writes {
            self.backend.write(write).await.map_err(|message| {
                self.connected = false;
                RnodeBleKissError::Backend { operation, message }
            })?;
        }
        Ok(())
    }
}

#[cfg(feature = "rnode-ble")]
#[derive(Debug, Clone)]
pub struct NativeRnodeBleKissInterface {
    label: String,
    settings: NativeRnodeBleSettings,
    config: RnodeBleKissConfig,
    rnode_config: Option<LoraConfig>,
    startup_response_timeout: Duration,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
}

#[cfg(feature = "rnode-ble")]
impl NativeRnodeBleKissInterface {
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        settings: NativeRnodeBleSettings,
        config: RnodeBleKissConfig,
    ) -> Self {
        Self {
            label: label.into(),
            settings,
            config,
            rnode_config: None,
            startup_response_timeout: Duration::from_millis(1_500),
            reconnect_backoff: Duration::from_millis(500),
            max_reconnect_backoff: Duration::from_millis(5_000),
        }
    }

    #[must_use]
    pub fn with_rnode_validation(
        mut self,
        rnode_config: LoraConfig,
        startup_response_timeout: Duration,
    ) -> Self {
        self.rnode_config = Some(rnode_config);
        self.startup_response_timeout = startup_response_timeout;
        self
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
        let (
            label,
            settings,
            config,
            rnode_config,
            startup_response_timeout,
            reconnect_backoff,
            max_reconnect_backoff,
        ) = {
            let guard = context.inner.lock().expect("RNode BLE interface mutex poisoned");
            (
                guard.label.clone(),
                guard.settings.clone(),
                guard.config.clone(),
                guard.rnode_config,
                guard.startup_response_timeout,
                guard.reconnect_backoff,
                guard.max_reconnect_backoff,
            )
        };
        let mut active_backoff = reconnect_backoff;

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let backend = NativeRnodeBleBackend::new(settings.clone());
            let mut runtime = RnodeBleKissRuntime::new(backend, config.clone());
            if let Err(err) = runtime.startup().await {
                log::warn!(
                    "RNode KISS-over-BLE session setup failed iface={} addr={} err={:?}",
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
            active_backoff = reconnect_backoff;
            log::info!(
                "RNode KISS-over-BLE session established iface={} addr={} peripheral_id={}",
                label,
                iface_address,
                settings.peripheral_id
            );

            let mut tx_buffer = vec![0_u8; config.mtu];
            let mut reconnect_needed = false;
            let mut command_monitor = rnode_config
                .map(|config| RnodeBleCommandMonitor::new(config, startup_response_timeout));
            let mut first_tx_at: Option<TokioInstant> = None;
            while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
                while let Ok(message) = tx_channel.try_recv() {
                    let mut output = OutputBuffer::new(&mut tx_buffer[..]);
                    if message.packet.serialize(&mut output).is_err() {
                        log::warn!("RNode BLE packet serialize failed iface={}", label);
                        continue;
                    }
                    if let Err(err) = runtime.send_packet(output.as_slice()).await {
                        log::warn!("RNode BLE packet write failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                    if first_tx_at.is_none() {
                        first_tx_at = Some(TokioInstant::now());
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
                                "RNode BLE station ID write failed iface={} err={:?}",
                                label,
                                err
                            );
                            reconnect_needed = true;
                            break;
                        }
                        first_tx_at = None;
                    }
                }

                match timeout(Duration::from_millis(100), runtime.poll_notification_events()).await
                {
                    Ok(Ok(notification)) => {
                        if let Some(monitor) = command_monitor.as_mut() {
                            if let Err(err) = monitor.accept_notification(&notification) {
                                log::warn!(
                                    "RNode BLE command response validation failed iface={} err={}",
                                    label,
                                    err
                                );
                                reconnect_needed = true;
                                break;
                            }
                        }
                        for payload in notification.packets {
                            if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(&payload))
                            {
                                let _ = rx_channel
                                    .send(RxMessage {
                                        address: iface_address,
                                        packet,
                                        source: IfaceSource::None,
                                    })
                                    .await;
                            }
                        }
                    }
                    Err(_) => {}
                    Ok(Err(err)) => {
                        log::warn!("RNode BLE packet read failed iface={} err={:?}", label, err);
                        reconnect_needed = true;
                        break;
                    }
                }
                if let Some(monitor) = command_monitor.as_mut() {
                    if let Err(err) = monitor.validate_startup_deadline() {
                        log::warn!(
                            "RNode BLE startup response validation failed iface={} err={}",
                            label,
                            err
                        );
                        reconnect_needed = true;
                        break;
                    }
                }
            }

            let _ = runtime.shutdown().await;
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

#[cfg(feature = "rnode-ble")]
impl Interface for NativeRnodeBleKissInterface {
    fn mtu() -> usize {
        508
    }
}

#[cfg(feature = "rnode-ble")]
fn bounded_backoff_next(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

#[derive(Debug, Clone)]
pub struct RnodeBleCommandMonitor {
    lora: LoraInterface,
    startup_deadline: Option<Instant>,
}

impl RnodeBleCommandMonitor {
    #[must_use]
    pub fn new(config: LoraConfig, startup_response_timeout: Duration) -> Self {
        let mut lora = LoraInterface::new_tcp("ble://rnode", config);
        lora.begin_startup_response_collection();
        Self { lora, startup_deadline: Some(Instant::now() + startup_response_timeout) }
    }

    pub fn accept_notification(
        &mut self,
        notification: &RnodeBleNotification,
    ) -> Result<(), String> {
        for (command, payload) in &notification.commands {
            let result = self.lora.record_command_response(*command, payload);
            let fatal = match &result {
                Ok(_) => false,
                Err(err) => self.lora.last_command_error() == Some(err.as_str()),
            };
            match (result, fatal) {
                (Ok(_), _) => {}
                (Err(err), true) => return Err(err),
                (Err(err), false) => {
                    log::warn!(
                        "ignored malformed RNode BLE command response command=0x{:02x} err={}",
                        command,
                        err
                    );
                }
            }
        }
        for _ in &notification.packets {
            self.lora.record_inbound_data_frame();
        }
        Ok(())
    }

    #[must_use]
    pub fn probe_status(&self) -> RNodeProbeStatus {
        self.lora.probe_status()
    }

    #[must_use]
    pub fn radio_status(&self) -> RNodeRadioStatus {
        self.lora.radio_status()
    }

    #[must_use]
    pub fn hardware_errors(&self) -> &[RNodeHardwareError] {
        self.lora.hardware_errors()
    }

    #[must_use]
    pub fn last_command_error(&self) -> Option<&str> {
        self.lora.last_command_error()
    }

    #[must_use]
    pub fn online(&self) -> bool {
        self.lora.online()
    }

    #[must_use]
    pub fn reported_bitrate_bps(&self) -> Option<f64> {
        self.lora.reported_bitrate_bps()
    }

    pub fn validate_startup_deadline(&mut self) -> Result<(), String> {
        let Some(deadline) = self.startup_deadline else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.startup_deadline = None;
        self.lora.validate_startup_responses()
    }
}

#[derive(Debug, Clone)]
pub struct RnodeBleKissSession {
    config: RnodeBleKissConfig,
    decoder: KissStreamDecoder,
    subscribed: bool,
    interface_ready: bool,
    last_read_at: Instant,
    pending_payloads: VecDeque<Vec<u8>>,
    pending_writes: VecDeque<RnodeBleWrite>,
}

impl RnodeBleKissSession {
    #[must_use]
    pub fn new(config: RnodeBleKissConfig) -> Self {
        Self {
            decoder: KissStreamDecoder::new(config.mtu),
            interface_ready: !config.kiss.flow_control,
            subscribed: false,
            last_read_at: Instant::now(),
            pending_payloads: VecDeque::new(),
            pending_writes: VecDeque::new(),
            config,
        }
    }

    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }

    #[must_use]
    pub fn status(&self) -> RnodeBleKissStatus {
        self.status_with_connection(false)
    }

    fn status_with_connection(&self, connected: bool) -> RnodeBleKissStatus {
        RnodeBleKissStatus {
            connected,
            subscribed: self.subscribed,
            interface_ready: self.interface_ready,
            pending_payloads: self.pending_payloads.len(),
            pending_writes: self.pending_writes.len(),
        }
    }

    #[must_use]
    pub fn pending_payloads(&self) -> usize {
        self.pending_payloads.len()
    }

    #[must_use]
    pub fn mtu(&self) -> usize {
        self.config.mtu
    }

    #[must_use]
    pub fn startup_frames(&mut self) -> Vec<RnodeBleWrite> {
        self.subscribed = true;
        self.config
            .kiss
            .command_frames()
            .into_iter()
            .chain(self.config.initial_frames.iter().cloned())
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn shutdown_frames(&self) -> Vec<RnodeBleWrite> {
        self.config
            .shutdown_frames
            .iter()
            .cloned()
            .flat_map(|frame| self.kiss_writes(frame))
            .collect()
    }

    #[must_use]
    pub fn enqueue_packet(&mut self, payload: &[u8]) -> Vec<RnodeBleWrite> {
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
    pub fn id_beacon_write(&self) -> Option<RnodeBleWrite> {
        self.config.kiss.id_beacon.as_ref().and_then(|beacon| {
            self.kiss_writes(encode_data_frame(&beacon.payload())).into_iter().next()
        })
    }

    #[must_use]
    pub fn enqueue_id_beacon(&mut self) -> Vec<RnodeBleWrite> {
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

    pub fn accept_notification(
        &mut self,
        payload: &[u8],
    ) -> Result<Vec<Vec<u8>>, RnodeBleKissError> {
        Ok(self.accept_notification_events(payload)?.packets)
    }

    pub fn accept_notification_events(
        &mut self,
        payload: &[u8],
    ) -> Result<RnodeBleNotification, RnodeBleKissError> {
        if self.decoder.has_partial_frame()
            && self.last_read_at.elapsed() >= self.config.read_frame_timeout
        {
            self.decoder.clear_partial_frame();
        }
        self.last_read_at = Instant::now();
        let frames = self.decoder.push_bytes(payload)?;
        let mut notification = RnodeBleNotification::default();
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
                        notification.packets.push(payload);
                    }
                }
                KissFrame::Command(KissCommand::Ready) => {
                    self.interface_ready = true;
                    self.flush_pending_payloads();
                }
                KissFrame::Command(KissCommand::Unknown(command, payload)) => {
                    notification.commands.push((command, payload));
                }
            }
        }
        Ok(notification)
    }

    #[must_use]
    pub fn take_pending_writes(&mut self) -> Vec<RnodeBleWrite> {
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

    fn kiss_writes(&self, payload: Vec<u8>) -> Vec<RnodeBleWrite> {
        let chunk_len = self.config.max_write_len.max(1);
        payload
            .chunks(chunk_len)
            .map(|chunk| RnodeBleWrite {
                characteristic_uuid: self.config.write_characteristic_uuid,
                with_response: self.config.write_with_response,
                payload: chunk.to_vec(),
            })
            .collect()
    }
}
