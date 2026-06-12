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

use rns_transport::iface::{IfaceSource, Interface, InterfaceContext, InterfaceManager, RxMessage};

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
                log::error!(
                    "establish session failed iface={} backend={} err={}",
                    label,
                    backend_name,
                    err.message
                );
                sleep(reconnect_backoff).await;
                reconnect_backoff = next_backoff(reconnect_backoff, settings.max_reconnect_backoff);
                continue;
            }
            reconnect_backoff = settings.reconnect_backoff;
            log::info!(
                "session established iface={} backend={} addr={}",
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
                            log::error!("packet serialize failed iface={}", label);
                            continue;
                        }
                        let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer);
                        if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_err() {
                            log::error!("hdlc encode failed iface={}", label);
                            continue;
                        }
                        if let Err(err) = send_chunked(&mut backend, hdlc_output.as_slice(), settings.mtu).await {
                            log::error!(
                                "write failed iface={} backend={} err={}",
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
                                            let _ = rx_channel.send(RxMessage { address: iface_address, packet, source: IfaceSource::None }).await;
                                        }
                                    }
                                    frame_buffer.drain(..=end);
                                }
                                if frame_buffer.len() > BLE_HDLC_BUFFER * 8 {
                                    frame_buffer.clear();
                                }
                            }
                            Err(err) => {
                                log::error!(
                                    "read failed iface={} backend={} err={}",
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
                reconnect_backoff = next_backoff(reconnect_backoff, settings.max_reconnect_backoff);
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
    log::info!(
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
