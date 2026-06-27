use super::{
    run_startup_lifecycle, BleBackend, BleBackendError, BleLifecyclePhase, BleLifecycleReport,
    BleRuntimeSettings, BleRuntimeStatus, BleRuntimeStatusHandle, BleSpawnResult,
};

use btleplug::api::{
    Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter,
    ValueNotification, WriteType,
};

use btleplug::platform::{Adapter, Manager, Peripheral};

use futures::{stream::Stream, StreamExt};

use reticulum_daemon::config::InterfaceConfig;

use rns_transport::buffer::{InputBuffer, OutputBuffer};

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
) -> Result<BleSpawnResult, String> {
    startup_with_backend(backend_name, iface, &settings).await?;

    let status = BleRuntimeStatusHandle::new(BleRuntimeStatus::from_settings(&settings));
    let interface = BleGattInterface {
        backend_name,
        settings,
        label: iface.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
        runtime_status: status.clone(),
    };

    let iface = iface_manager
        .lock()
        .await
        .spawn(interface, |context| async move { BleGattInterface::run(context).await });
    status.update(|status| {
        status.iface = Some(iface.to_string());
    });
    Ok(BleSpawnResult { iface, status })
}

const BLE_RAW_PACKET_BUFFER: usize = 4096;

const BLE_HDLC_BUFFER: usize = 8192;

struct BleGattInterface {
    backend_name: &'static str,
    settings: BleRuntimeSettings,
    label: String,
    runtime_status: BleRuntimeStatusHandle,
}

impl BleGattInterface {
    async fn run(context: InterfaceContext<Self>) {
        let iface_stop = context.channel.stop.clone();
        let iface_address = context.channel.address;
        let (rx_channel, mut tx_channel) = context.channel.split();
        let (backend_name, settings, label, runtime_status) = {
            let guard = context.inner.lock().expect("ble interface mutex poisoned");
            (
                guard.backend_name,
                guard.settings.clone(),
                guard.label.clone(),
                guard.runtime_status.clone(),
            )
        };
        runtime_status.update(|status| {
            status.iface = Some(iface_address.to_string());
            status.link_state = "opening".to_string();
            status.last_error = None;
        });

        let mut reconnect_backoff = settings.reconnect_backoff;
        let mut frame_buffer: Vec<u8> = Vec::with_capacity(BLE_HDLC_BUFFER * 2);

        loop {
            if context.cancel.is_cancelled() {
                break;
            }

            let mut backend = NativeBleBackend::new(backend_name);
            if let Err(err) = establish_session(&mut backend, &settings, &runtime_status).await {
                log::error!(
                    "establish session failed iface={} backend={} err={}",
                    label,
                    backend_name,
                    err.message
                );
                runtime_status.update(|status| {
                    status.reconnect_attempts = status.reconnect_attempts.saturating_add(1);
                });
                sleep(reconnect_backoff).await;
                reconnect_backoff = next_backoff(reconnect_backoff, settings.max_reconnect_backoff);
                continue;
            }
            reconnect_backoff = settings.reconnect_backoff;
            runtime_status.update(|status| {
                status.link_state = "running".to_string();
                status.connected = true;
                status.subscribed = true;
                status.last_error = None;
            });
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
                            runtime_status.update(|status| {
                                status.serialize_errors =
                                    status.serialize_errors.saturating_add(1);
                                status.last_error = Some("packet serialize failed".to_string());
                            });
                            continue;
                        }
                        let mut hdlc_output = OutputBuffer::new(&mut hdlc_tx_buffer);
                        if Hdlc::encode(output.as_slice(), &mut hdlc_output).is_err() {
                            log::error!("hdlc encode failed iface={}", label);
                            runtime_status.update(|status| {
                                status.hdlc_encode_errors =
                                    status.hdlc_encode_errors.saturating_add(1);
                                status.last_error = Some("HDLC encode failed".to_string());
                            });
                            continue;
                        }
                        let hdlc_len = hdlc_output.as_slice().len();
                        match send_chunked(&mut backend, hdlc_output.as_slice(), settings.mtu).await {
                            Ok(chunks) => {
                                runtime_status.update(|status| {
                                    status.link_state = "running".to_string();
                                    status.packets_tx = status.packets_tx.saturating_add(1);
                                    status.frames_tx = status.frames_tx.saturating_add(1);
                                    status.bytes_tx =
                                        status.bytes_tx.saturating_add(hdlc_len as u64);
                                    status.write_chunks_tx =
                                        status.write_chunks_tx.saturating_add(chunks as u64);
                                    status.last_error = None;
                                });
                            }
                            Err(err) => {
                            log::error!(
                                "write failed iface={} backend={} err={}",
                                label, backend_name, err.message
                            );
                                runtime_status.update(|status| {
                                    status.link_state = "write_error".to_string();
                                    status.write_errors = status.write_errors.saturating_add(1);
                                    status.reconnect_attempts =
                                        status.reconnect_attempts.saturating_add(1);
                                    status.last_error = Some(err.message.clone());
                                });
                            reconnect_needed = true;
                            break;
                            }
                        }
                    }
                    notification = backend.read_notification_value(&settings) => {
                        match notification {
                            Ok(payload) => {
                                runtime_status.update(|status| {
                                    status.link_state = "running".to_string();
                                    status.notification_bytes_rx = status
                                        .notification_bytes_rx
                                        .saturating_add(payload.len() as u64);
                                    status.last_error = None;
                                });
                                frame_buffer.extend_from_slice(payload.as_slice());
                                while let Some((start, end)) = Hdlc::find(&frame_buffer) {
                                    let frame = &frame_buffer[start..=end];
                                    let mut output = OutputBuffer::new(&mut hdlc_rx_buffer);
                                    match Hdlc::decode(frame, &mut output) {
                                        Ok(_) => {
                                            runtime_status.update(|status| {
                                                status.frames_rx =
                                                    status.frames_rx.saturating_add(1);
                                                status.bytes_rx = status
                                                    .bytes_rx
                                                    .saturating_add(output.as_slice().len() as u64);
                                            });
                                            match Packet::deserialize(&mut InputBuffer::new(
                                                output.as_slice(),
                                            )) {
                                                Ok(packet) => {
                                                    if let Err(err) = rx_channel
                                                        .send(RxMessage {
                                                            address: iface_address,
                                                            packet,
                                                            source: IfaceSource::None,
                                                        })
                                                        .await
                                                    {
                                                log::warn!("BLE RX queue closed iface={} err={err}", label);
                                                        runtime_status.update(|status| {
                                                            status.rx_queue_errors = status
                                                                .rx_queue_errors
                                                                .saturating_add(1);
                                                            status.last_error =
                                                                Some(err.to_string());
                                                        });
                                                    } else {
                                                        runtime_status.update(|status| {
                                                            status.packets_rx = status
                                                                .packets_rx
                                                                .saturating_add(1);
                                                            status.last_error = None;
                                                        });
                                                    }
                                                }
                                                Err(_) => {
                                                    runtime_status.update(|status| {
                                                        status.deserialize_errors = status
                                                            .deserialize_errors
                                                            .saturating_add(1);
                                                        status.last_error = Some(
                                                            "packet deserialize failed".to_string(),
                                                        );
                                                    });
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            runtime_status.update(|status| {
                                                status.hdlc_decode_errors =
                                                    status.hdlc_decode_errors.saturating_add(1);
                                                status.last_error =
                                                    Some("HDLC decode failed".to_string());
                                            });
                                        }
                                    }
                                    frame_buffer.drain(..=end);
                                }
                                if frame_buffer.len() > BLE_HDLC_BUFFER * 8 {
                                    frame_buffer.clear();
                                    runtime_status.update(|status| {
                                        status.stale_buffer_drops =
                                            status.stale_buffer_drops.saturating_add(1);
                                        status.last_error =
                                            Some("BLE HDLC receive buffer overflow".to_string());
                                    });
                                }
                            }
                            Err(err) => {
                                log::error!(
                                    "read failed iface={} backend={} err={}",
                                    label, backend_name, err.message
                                );
                                runtime_status.update(|status| {
                                    status.link_state = "read_error".to_string();
                                    status.read_errors = status.read_errors.saturating_add(1);
                                    status.reconnect_attempts =
                                        status.reconnect_attempts.saturating_add(1);
                                    status.last_error = Some(err.message.clone());
                                });
                                reconnect_needed = true;
                                break;
                            }
                        }
                    }
                }
            }

            if let Err(err) = backend.cleanup(&settings).await {
                runtime_status.update(|status| {
                    status.cleanup_errors = status.cleanup_errors.saturating_add(1);
                    status.last_error = Some(err.message.clone());
                });
            }
            runtime_status.update(|status| {
                status.connected = false;
                status.subscribed = false;
                if status.link_state == "running" {
                    status.link_state = "closed".to_string();
                }
            });
            if context.cancel.is_cancelled() {
                break;
            }
            if reconnect_needed {
                sleep(reconnect_backoff).await;
                reconnect_backoff = next_backoff(reconnect_backoff, settings.max_reconnect_backoff);
            }
        }

        iface_stop.cancel();
        runtime_status.update(|status| {
            status.link_state = "stopped".to_string();
            status.connected = false;
            status.subscribed = false;
        });
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
    status: &BleRuntimeStatusHandle,
) -> Result<(), BleBackendError> {
    backend.scan(settings).await.inspect_err(|err| {
        record_ble_session_error(status, BleLifecyclePhase::Scan, err);
    })?;
    backend.connect(settings).await.inspect_err(|err| {
        record_ble_session_error(status, BleLifecyclePhase::Connect, err);
    })?;
    backend.subscribe(settings).await.inspect_err(|err| {
        record_ble_session_error(status, BleLifecyclePhase::Subscribe, err);
    })?;
    backend
        .write_probe(super::BLE_STARTUP_PROBE_PAYLOAD, settings)
        .await
        .inspect_err(|err| {
            record_ble_session_error(status, BleLifecyclePhase::WriteProbe, err);
        })?;
    let probe = backend.read_probe_notification(settings).await.inspect_err(|err| {
        record_ble_session_error(status, BleLifecyclePhase::NotificationProbe, err);
    })?;
    if probe != super::BLE_STARTUP_PROBE_PAYLOAD {
        let err = BleBackendError::terminal(format!(
            "probe payload mismatch expected={} actual={}",
            super::BLE_STARTUP_PROBE_PAYLOAD.len(),
            probe.len()
        ));
        record_ble_session_error(status, BleLifecyclePhase::NotificationProbe, &err);
        return Err(err);
    }
    Ok(())
}

async fn send_chunked(
    backend: &mut NativeBleBackend,
    payload: &[u8],
    mtu: usize,
) -> Result<usize, BleBackendError> {
    let chunk_size = mtu.clamp(23, 517).saturating_sub(3).max(20);
    let mut chunks = 0_usize;
    for chunk in payload.chunks(chunk_size) {
        backend.write_payload(chunk).await?;
        chunks += 1;
    }
    Ok(chunks)
}

fn record_ble_session_error(
    status: &BleRuntimeStatusHandle,
    phase: BleLifecyclePhase,
    err: &BleBackendError,
) {
    status.update(|status| {
        status.link_state = format!("{}_failed", phase.as_str());
        status.connected = false;
        status.subscribed = false;
        status.last_error = Some(err.message.clone());
        match phase {
            BleLifecyclePhase::Scan => {
                status.scan_errors = status.scan_errors.saturating_add(1);
            }
            BleLifecyclePhase::Connect => {
                status.connect_errors = status.connect_errors.saturating_add(1);
            }
            BleLifecyclePhase::Subscribe => {
                status.subscribe_errors = status.subscribe_errors.saturating_add(1);
            }
            BleLifecyclePhase::WriteProbe => {
                status.probe_write_errors = status.probe_write_errors.saturating_add(1);
            }
            BleLifecyclePhase::NotificationProbe => {
                status.probe_read_errors = status.probe_read_errors.saturating_add(1);
            }
        }
    });
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
