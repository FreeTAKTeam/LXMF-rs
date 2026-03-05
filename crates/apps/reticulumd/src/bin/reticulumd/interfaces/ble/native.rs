use super::{
    run_startup_lifecycle, BleBackend, BleBackendError, BleLifecycleReport, BleRuntimeSettings,
};
use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::{stream::Stream, StreamExt};
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::buffer::{InputBuffer, OutputBuffer};
use rns_transport::hash::AddressHash;
use rns_transport::iface::hdlc::Hdlc;
use rns_transport::iface::{Interface, InterfaceContext, InterfaceManager, RxMessage};
use rns_transport::packet::Packet;
use rns_transport::serde::Serialize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

type NotificationStream = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

const SCAN_POLL_INTERVAL: Duration = Duration::from_millis(200);

pub(super) async fn startup_with_backend(
    backend_name: &'static str,
    iface: &InterfaceConfig,
    settings: &BleRuntimeSettings,
) -> Result<(), String> {
    let mut backend = NativeBleBackend::new(backend_name);
    let report = run_startup_lifecycle(&mut backend, settings).await?;
    log_report(backend_name, iface, settings, &report);
    Ok(())
}

pub(super) async fn spawn_with_backend(
    backend_name: &'static str,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    iface: &InterfaceConfig,
    settings: BleRuntimeSettings,
) -> Result<AddressHash, String> {
    startup_with_backend(backend_name, iface, &settings).await?;

    let interface = BleGattInterface {
        backend_name,
        settings,
        label: iface.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
    };

    let iface = iface_manager
        .lock()
        .await
        .spawn(interface, |context| async move { BleGattInterface::run(context).await });
    Ok(iface)
}

const BLE_RAW_PACKET_BUFFER: usize = 4096;
const BLE_HDLC_BUFFER: usize = 8192;

struct BleGattInterface {
    backend_name: &'static str,
    settings: BleRuntimeSettings,
    label: String,
}

impl BleGattInterface {
    async fn run(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (backend_name, settings, label) = {
            let guard = context.inner.lock().expect("ble interface mutex poisoned");
            (guard.backend_name, guard.settings.clone(), guard.label.clone())
        };

        let mut reconnect_backoff = settings.reconnect_backoff;
        let mut frame_buffer: Vec<u8> = Vec::with_capacity(BLE_HDLC_BUFFER * 2);

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let mut backend = NativeBleBackend::new(backend_name);
            if let Err(err) = establish_session(&mut backend, &settings).await {
                eprintln!(
                    "ble_gatt: establish session failed iface={} backend={} err={}",
                    label,
                    backend_name,
                    err.message
                );
                sleep(reconnect_backoff).await;
                reconnect_backoff =
                    next_backoff(reconnect_backoff, settings.max_reconnect_backoff);
                continue;
            }
            reconnect_backoff = settings.reconnect_backoff;
            eprintln!(
                "ble_gatt: session established iface={} backend={} addr={}",
                label,
                backend_name,
                iface_address
            );

            let mut tx_buffer = [0_u8; BLE_RAW_PACKET_BUFFER];
            let mut hdlc_tx_buffer = [0_u8; BLE_HDLC_BUFFER];
            let mut hdlc_rx_buffer = [0_u8; BLE_RAW_PACKET_BUFFER];
            let mut reconnect_needed = false;

            while !context.cancel.is_cancelled() && !iface_stop.is_cancelled() {
                tokio::select! {
                    _ = context.cancel.cancelled() => {
                        break;
                    }
                    Some(message) = tx_channel.recv() => {
                        let packet = message.packet;
                        let mut output = OutputBuffer::new(&mut tx_buffer);
                        if packet.serialize(&mut output).is_err() {
                            eprintln!("ble_gatt: packet serialize failed iface={}", label);
                            continue;
                        }
                        let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer);
                        if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_err() {
                            eprintln!("ble_gatt: hdlc encode failed iface={}", label);
                            continue;
                        }
                        if let Err(err) = send_chunked(&mut backend, hdlc_output.as_slice(), settings.mtu).await {
                            eprintln!(
                                "ble_gatt: write failed iface={} backend={} err={}",
                                label, backend_name, err.message
                            );
                            reconnect_needed = true;
                            break;
                        }
                    }
                    notification = backend.read_notification_value(&settings) => {
                        match notification {
                            Ok(payload) => {
                                frame_buffer.extend_from_slice(payload.as_slice());
                                while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                    let frame = &frame_buffer[start..=end];
                                    let mut output = OutputBuffer::new(&mut hdlc_rx_buffer);
                                    if Hdlc::decode(frame, &mut output).is_ok() {
                                        if let Ok(packet) = Packet::deserialize(&mut InputBuffer::new(output.as_slice())) {
                                            let _ = rx_channel.send(RxMessage { address: iface_address, packet }).await;
                                        }
                                    }
                                    frame_buffer.drain(..=end);
                                }
                                if frame_buffer.len() > BLE_HDLC_BUFFER * 8 {
                                    frame_buffer.clear();
                                }
                            }
                            Err(err) => {
                                eprintln!(
                                    "ble_gatt: read failed iface={} backend={} err={}",
                                    label, backend_name, err.message
                                );
                                reconnect_needed = true;
                                break;
                            }
                        }
                    }
                }
            }

            let _ = backend.cleanup(&settings).await;
            if context.cancel.is_cancelled() {
                break;
            }
            if reconnect_needed {
                sleep(reconnect_backoff).await;
                reconnect_backoff =
                    next_backoff(reconnect_backoff, settings.max_reconnect_backoff);
            }
        }

        iface_stop.cancel();
    }
}

impl Interface for BleGattInterface {
    fn mtu() -> usize {
        247
    }
}

async fn establish_session(
    backend: &mut NativeBleBackend,
    settings: &BleRuntimeSettings,
) -> Result<(), BleBackendError> {
    backend.scan(settings).await?;
    backend.connect(settings).await?;
    backend.subscribe(settings).await?;
    backend.write_probe(super::BLE_STARTUP_PROBE_PAYLOAD, settings).await?;
    let probe = backend.read_probe_notification(settings).await?;
    if probe != super::BLE_STARTUP_PROBE_PAYLOAD {
        return Err(BleBackendError::terminal(format!(
            "probe payload mismatch expected={} actual={}",
            super::BLE_STARTUP_PROBE_PAYLOAD.len(),
            probe.len()
        )));
    }
    Ok(())
}

async fn send_chunked(
    backend: &mut NativeBleBackend,
    payload: &[u8],
    mtu: usize,
) -> Result<(), BleBackendError> {
    let chunk_size = mtu.clamp(23, 517).saturating_sub(3).max(20);
    for chunk in payload.chunks(chunk_size) {
        backend.write_payload(chunk).await?;
    }
    Ok(())
}

fn next_backoff(current: Duration, max: Duration) -> Duration {
    let current_ms = current.as_millis() as u64;
    let max_ms = max.as_millis() as u64;
    Duration::from_millis(current_ms.saturating_mul(2).min(max_ms))
}

fn log_report(
    backend_name: &str,
    iface: &InterfaceConfig,
    settings: &BleRuntimeSettings,
    report: &BleLifecycleReport,
) {
    eprintln!(
        "[daemon] ble_gatt configured ({} backend) name={} adapter={} peripheral_id={} service_uuid={} write_char_uuid={} notify_char_uuid={} mtu={} scan_timeout_ms={} connect_timeout_ms={} reconnect_backoff_ms={} max_reconnect_backoff_ms={} attempts={} transitions={}",
        backend_name,
        iface.name.as_deref().unwrap_or("<unnamed>"),
        settings.adapter.as_deref().unwrap_or("<default>"),
        settings.peripheral_id,
        settings.service_uuid,
        settings.write_char_uuid,
        settings.notify_char_uuid,
        settings.mtu,
        settings.scan_timeout.as_millis(),
        settings.connect_timeout.as_millis(),
        settings.reconnect_backoff.as_millis(),
        settings.max_reconnect_backoff.as_millis(),
        report.attempts,
        report.transitions.len(),
    );
}

struct NativeBleBackend {
    backend_name: &'static str,
    adapter: Option<Adapter>,
    peripheral: Option<Peripheral>,
    write_char: Option<Characteristic>,
    notify_char: Option<Characteristic>,
    notification_stream: Option<NotificationStream>,
    write_type: Option<WriteType>,
}

impl NativeBleBackend {
    fn new(backend_name: &'static str) -> Self {
        Self {
            backend_name,
            adapter: None,
            peripheral: None,
            write_char: None,
            notify_char: None,
            notification_stream: None,
            write_type: None,
        }
    }

    fn clear_session_state(&mut self) {
        self.adapter = None;
        self.peripheral = None;
        self.write_char = None;
        self.notify_char = None;
        self.notification_stream = None;
        self.write_type = None;
    }

    async fn select_adapter(settings: &BleRuntimeSettings) -> Result<Adapter, BleBackendError> {
        let manager = Manager::new()
            .await
            .map_err(|err| BleBackendError::retryable(format!("create BLE manager: {err}")))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|err| BleBackendError::retryable(format!("enumerate BLE adapters: {err}")))?;

        if adapters.is_empty() {
            return Err(BleBackendError::retryable("no BLE adapters available on host"));
        }

        if let Some(requested) = settings.adapter.as_deref() {
            let requested = requested.trim();
            let mut adapter_lookup_errors = Vec::new();
            for adapter in adapters {
                match adapter_matches(&adapter, requested).await {
                    Ok(true) => return Ok(adapter),
                    Ok(false) => {}
                    Err(err) => adapter_lookup_errors.push(err.message),
                }
            }
            if !adapter_lookup_errors.is_empty() {
                return Err(BleBackendError::retryable(format!(
                    "failed to inspect adapters while looking up '{}': {}",
                    requested,
                    adapter_lookup_errors.join("; ")
                )));
            }
            return Err(BleBackendError::terminal(format!(
                "configured adapter '{}' not found",
                requested
            )));
        }

        Ok(adapters.into_iter().next().expect("adapters already checked as non-empty"))
    }

    fn resolve_characteristics(
        &self,
        settings: &BleRuntimeSettings,
    ) -> Result<(Characteristic, Characteristic, WriteType), BleBackendError> {
        let peripheral = self.peripheral.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not select a peripheral")
        })?;
        let service_uuid = parse_gatt_uuid("service_uuid", &settings.service_uuid)?;
        let write_uuid = parse_gatt_uuid("write_char_uuid", &settings.write_char_uuid)?;
        let notify_uuid = parse_gatt_uuid("notify_char_uuid", &settings.notify_char_uuid)?;

        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == write_uuid)
            .cloned()
            .ok_or_else(|| {
                BleBackendError::terminal(format!(
                    "write characteristic {} not found on peripheral",
                    settings.write_char_uuid
                ))
            })?;

        let notify_char = characteristics
            .iter()
            .find(|characteristic| characteristic.uuid == notify_uuid)
            .cloned()
            .ok_or_else(|| {
                BleBackendError::terminal(format!(
                    "notify characteristic {} not found on peripheral",
                    settings.notify_char_uuid
                ))
            })?;

        if write_char.service_uuid != service_uuid {
            return Err(BleBackendError::terminal(format!(
                "write characteristic {} does not belong to service {}",
                settings.write_char_uuid, settings.service_uuid
            )));
        }
        if notify_char.service_uuid != service_uuid {
            return Err(BleBackendError::terminal(format!(
                "notify characteristic {} does not belong to service {}",
                settings.notify_char_uuid, settings.service_uuid
            )));
        }

        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(BleBackendError::terminal(format!(
                "write characteristic {} does not support write operations",
                settings.write_char_uuid
            )));
        };

        if !notify_char.properties.contains(CharPropFlags::NOTIFY)
            && !notify_char.properties.contains(CharPropFlags::INDICATE)
        {
            return Err(BleBackendError::terminal(format!(
                "notify characteristic {} does not support notifications or indications",
                settings.notify_char_uuid
            )));
        }

        Ok((write_char, notify_char, write_type))
    }

    async fn find_peripheral(
        adapter: &Adapter,
        settings: &BleRuntimeSettings,
    ) -> Result<Peripheral, BleBackendError> {
        let deadline = Instant::now() + settings.scan_timeout;
        loop {
            let peripherals = adapter
                .peripherals()
                .await
                .map_err(|err| BleBackendError::retryable(format!("list peripherals: {err}")))?;

            for peripheral in peripherals {
                if peripheral_matches(&peripheral, &settings.peripheral_id).await? {
                    return Ok(peripheral);
                }
            }

            if Instant::now() >= deadline {
                return Err(BleBackendError::retryable(format!(
                    "scan timeout waiting for peripheral_id={}",
                    settings.peripheral_id
                )));
            }

            sleep(SCAN_POLL_INTERVAL).await;
        }
    }

    async fn write_payload(&mut self, payload: &[u8]) -> Result<(), BleBackendError> {
        let peripheral = self.peripheral.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not provide a peripheral")
        })?;
        let write_char = self.write_char.clone().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not resolve write characteristic")
        })?;
        let write_type = self
            .write_type
            .ok_or_else(|| BleBackendError::terminal("connect phase did not resolve write mode"))?;

        peripheral
            .write(&write_char, payload, write_type)
            .await
            .map_err(|err| BleBackendError::retryable(format!("write payload: {err}")))
    }

    async fn read_notification_value(
        &mut self,
        settings: &BleRuntimeSettings,
    ) -> Result<Vec<u8>, BleBackendError> {
        let notify_char = self.notify_char.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not resolve notify characteristic")
        })?;
        let notify_uuid = notify_char.uuid;

        let stream = self.notification_stream.as_mut().ok_or_else(|| {
            BleBackendError::terminal("subscribe phase did not initialize notification stream")
        })?;

        let notification = timeout(settings.connect_timeout, stream.as_mut().next())
            .await
            .map_err(|_| {
                BleBackendError::retryable(format!(
                    "notification timeout after {} ms",
                    settings.connect_timeout.as_millis()
                ))
            })?
            .ok_or_else(|| BleBackendError::retryable("notification stream closed"))?;

        if notification.uuid != notify_uuid {
            return Err(BleBackendError::retryable(
                "notification for unexpected characteristic",
            ));
        }

        Ok(notification.value)
    }
}

impl BleBackend for NativeBleBackend {
    fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    async fn scan(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        if settings
            .adapter
            .as_deref()
            .is_some_and(|adapter| adapter.eq_ignore_ascii_case("disabled"))
        {
            return Err(BleBackendError::terminal(format!(
                "{} BLE adapter is disabled by configuration",
                self.backend_name
            )));
        }

        self.clear_session_state();
        let adapter = Self::select_adapter(settings).await?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|err| BleBackendError::retryable(format!("start BLE scan: {err}")))?;
        self.adapter = Some(adapter.clone());

        let peripheral = Self::find_peripheral(&adapter, settings).await?;
        self.peripheral = Some(peripheral);
        Ok(())
    }

    async fn connect(&mut self, settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        let peripheral = self
            .peripheral
            .as_ref()
            .ok_or_else(|| BleBackendError::terminal("scan phase did not identify a peripheral"))?
            .clone();

        timeout(settings.connect_timeout, async {
            let connected = peripheral.is_connected().await.map_err(|err| {
                BleBackendError::retryable(format!("read BLE connection state: {err}"))
            })?;
            if !connected {
                peripheral.connect().await.map_err(|err| {
                    BleBackendError::retryable(format!("connect peripheral: {err}"))
                })?;
            }
            peripheral
                .discover_services()
                .await
                .map_err(|err| BleBackendError::retryable(format!("discover GATT services: {err}")))
        })
        .await
        .map_err(|_| {
            BleBackendError::retryable(format!(
                "connect timeout after {} ms",
                settings.connect_timeout.as_millis()
            ))
        })??;

        let (write_char, notify_char, write_type) = self.resolve_characteristics(settings)?;
        self.write_char = Some(write_char);
        self.notify_char = Some(notify_char);
        self.write_type = Some(write_type);
        Ok(())
    }

    async fn subscribe(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        let peripheral = self.peripheral.as_ref().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not provide a peripheral")
        })?;
        let notify_char = self.notify_char.clone().ok_or_else(|| {
            BleBackendError::terminal("connect phase did not resolve notify characteristic")
        })?;

        let stream = peripheral.notifications().await.map_err(|err| {
            BleBackendError::retryable(format!("open notification stream: {err}"))
        })?;
        self.notification_stream = Some(Box::pin(stream));

        peripheral.subscribe(&notify_char).await.map_err(|err| {
            BleBackendError::retryable(format!("subscribe to notify characteristic: {err}"))
        })
    }

    async fn write_probe(
        &mut self,
        payload: &[u8],
        _settings: &BleRuntimeSettings,
    ) -> Result<(), BleBackendError> {
        self.write_payload(payload)
            .await
            .map_err(|err| BleBackendError::retryable(format!("write probe payload: {}", err.message)))
    }

    async fn read_probe_notification(
        &mut self,
        settings: &BleRuntimeSettings,
    ) -> Result<Vec<u8>, BleBackendError> {
        self.read_notification_value(settings)
            .await
            .map_err(|err| {
                BleBackendError::retryable(format!(
                    "probe notification read failed: {}",
                    err.message
                ))
            })
    }

    async fn cleanup(&mut self, _settings: &BleRuntimeSettings) -> Result<(), BleBackendError> {
        let mut failures = Vec::new();

        if let (Some(peripheral), Some(notify_char)) =
            (self.peripheral.as_ref(), self.notify_char.as_ref())
        {
            if let Err(err) = peripheral.unsubscribe(notify_char).await {
                failures.push(format!("unsubscribe notify characteristic: {err}"));
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
                Err(err) => {
                    failures.push(format!("read BLE connection state during cleanup: {err}"))
                }
            }
        }

        self.clear_session_state();

        if failures.is_empty() {
            Ok(())
        } else {
            Err(BleBackendError::retryable(format!(
                "native BLE cleanup encountered errors: {}",
                failures.join("; ")
            )))
        }
    }
}

async fn adapter_matches(adapter: &Adapter, requested: &str) -> Result<bool, BleBackendError> {
    let adapter_info = adapter
        .adapter_info()
        .await
        .map_err(|err| BleBackendError::retryable(format!("read adapter info: {err}")))?;

    if identifiers_match(requested, &adapter_info) {
        return Ok(true);
    }

    let peripherals = adapter
        .peripherals()
        .await
        .map_err(|err| BleBackendError::retryable(format!("list adapter peripherals: {err}")))?;
    for peripheral in peripherals {
        if identifiers_match(requested, &peripheral.id().to_string()) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
) -> Result<bool, BleBackendError> {
    if identifiers_match(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }

    let properties = peripheral
        .properties()
        .await
        .map_err(|err| BleBackendError::retryable(format!("read peripheral properties: {err}")))?;

    if let Some(properties) = properties {
        if identifiers_match(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if identifiers_match(configured_id, &local_name) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn identifiers_match(configured: &str, discovered: &str) -> bool {
    normalize_identifier(configured) == normalize_identifier(discovered)
}

fn normalize_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn parse_gatt_uuid(field: &str, value: &str) -> Result<Uuid, BleBackendError> {
    let normalized = value.trim();
    if normalized.len() == 4 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("0000{normalized}-0000-1000-8000-00805f9b34fb")).map_err(
            |err| BleBackendError::terminal(format!("invalid ble_gatt.{field} '{value}': {err}")),
        );
    }
    if normalized.len() == 8 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("{normalized}-0000-1000-8000-00805f9b34fb")).map_err(
            |err| BleBackendError::terminal(format!("invalid ble_gatt.{field} '{value}': {err}")),
        );
    }

    Uuid::parse_str(normalized).map_err(|err| {
        BleBackendError::terminal(format!("invalid ble_gatt.{field} '{value}': {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    fn settings_with_adapter(adapter: Option<&str>) -> BleRuntimeSettings {
        BleRuntimeSettings {
            adapter: adapter.map(ToOwned::to_owned),
            peripheral_id: "AA:BB:CC:DD:EE:FF".to_string(),
            service_uuid: "12345678-1234-1234-1234-1234567890ab".to_string(),
            write_char_uuid: "2A37".to_string(),
            notify_char_uuid: "2A38".to_string(),
            mtu: 247,
            scan_timeout: Duration::from_millis(100),
            connect_timeout: Duration::from_millis(100),
            reconnect_backoff: Duration::from_millis(50),
            max_reconnect_backoff: Duration::from_millis(100),
        }
    }

    #[test]
    fn identifiers_match_normalizes_case_and_separators() {
        assert!(identifiers_match("AA:BB:CC:DD", "aabbccdd"));
        assert!(identifiers_match("AB-CD-EF", "abcdef"));
        assert!(!identifiers_match("AB-CD-EF", "abcdee"));
    }

    #[test]
    fn parse_gatt_uuid_accepts_short_and_full_forms() {
        assert_eq!(
            parse_gatt_uuid("write_char_uuid", "2A37").expect("16-bit UUID").to_string(),
            "00002a37-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            parse_gatt_uuid("write_char_uuid", "12345678").expect("32-bit UUID").to_string(),
            "12345678-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            parse_gatt_uuid("write_char_uuid", "12345678-1234-1234-1234-1234567890ab")
                .expect("128-bit UUID")
                .to_string(),
            "12345678-1234-1234-1234-1234567890ab"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[tokio::test(flavor = "current_thread")]
    async fn native_scan_with_unknown_adapter_exercises_adapter_selection_path() {
        let mut backend = NativeBleBackend::new("native-test");
        let settings = settings_with_adapter(Some("__adapter_that_should_not_exist__"));
        let err = backend.scan(&settings).await.expect_err("unknown adapter should fail scan");

        assert!(
            err.message.contains("configured adapter")
                || err.message.contains("no BLE adapters available")
                || err.message.contains("create BLE manager")
                || err.message.contains("read adapter info")
                || err.message.contains("enumerate BLE adapters"),
            "unexpected scan failure reason: {}",
            err.message
        );
    }
}
