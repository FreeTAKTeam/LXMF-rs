#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::platform::{Manager, Peripheral};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use futures::StreamExt;

use std::io;
use std::time::{Duration, Instant};

use crate::{
    embedded_to_io, hex_lower, parse_hex_16, resolve_runtime_seq, upload_attachment_via_rpc,
    NativePeerMode,
};
use rns_embedded_core::{
    lxmf_min::{decode_envelope, encode_envelope, MinimalEnvelope},
    packet::{decode_frame, encode_frame, PacketFrame},
};
use rns_embedded_runtime::{
    BLE_FRAME_NATIVE_WIRE, FRAME_KIND_ANNOUNCE, FRAME_KIND_LXMF_MESSAGE, FRAME_KIND_TEST_PING,
};

use super::helpers::{find_peripheral, parse_gatt_uuid};

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) fn run_ble_native_peer(
    scan_secs: u64,
    name_hint: String,
    peripheral_id: Option<String>,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
    mode: NativePeerMode,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    timeout_secs: u64,
) -> io::Result<()> {
    let scan_timeout = Duration::from_secs(scan_secs.max(1));
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let service_uuid = parse_gatt_uuid(service_uuid.as_str())?;
    let write_uuid = parse_gatt_uuid(write_char_uuid.as_str())?;
    let notify_uuid = parse_gatt_uuid(notify_char_uuid.as_str())?;
    let device_hint = peripheral_id.unwrap_or_else(|| name_hint.clone());
    let runtime_seq = resolve_runtime_seq(runtime_seq);
    let payload_bytes = payload.into_bytes();
    let runtime_frame = match mode {
        NativePeerMode::RawPing => {
            let frame = PacketFrame::new(FRAME_KIND_TEST_PING, runtime_seq, payload_bytes)
                .map_err(embedded_to_io)?;
            encode_frame(&frame).map_err(embedded_to_io)?
        }
        NativePeerMode::LxmfPing => {
            let source = parse_hex_16(source_hex.as_str())?;
            let destination = parse_hex_16(destination_hex.as_str())?;
            let envelope = MinimalEnvelope {
                source,
                destination,
                sequence: u64::from(runtime_seq),
                body: payload_bytes,
            };
            let frame = PacketFrame::new(
                FRAME_KIND_LXMF_MESSAGE,
                runtime_seq,
                encode_envelope(&envelope).map_err(embedded_to_io)?,
            )
            .map_err(embedded_to_io)?;
            encode_frame(&frame).map_err(embedded_to_io)?
        }
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;
        let peripheral: Peripheral =
            find_peripheral(&adapter, device_hint.as_str(), Some(service_uuid), scan_timeout)
                .await?;

        let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
        if !connected {
            peripheral.connect().await.map_err(io::Error::other)?;
        }
        peripheral.discover_services().await.map_err(io::Error::other)?;
        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("write characteristic not found"))?;
        let notify_char = characteristics
            .iter()
            .find(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("notify characteristic not found"))?;
        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(io::Error::other("write characteristic has no write capability"));
        };

        let mut notifications = peripheral.notifications().await.map_err(io::Error::other)?;
        peripheral.subscribe(&notify_char).await.map_err(io::Error::other)?;

        let mut outbound = Vec::with_capacity(1 + runtime_frame.len());
        outbound.push(BLE_FRAME_NATIVE_WIRE);
        outbound.extend_from_slice(&runtime_frame);
        peripheral
            .write(&write_char, outbound.as_slice(), write_type)
            .await
            .map_err(io::Error::other)?;

        let deadline = Instant::now() + timeout;
        let mut responses = 0usize;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let notification = match tokio::time::timeout(remaining, notifications.next()).await {
                Ok(Some(notification)) => notification,
                Ok(None) => break,
                Err(_) => break,
            };
            if notification.uuid != notify_uuid || notification.value.is_empty() {
                continue;
            }
            if notification.value[0] != BLE_FRAME_NATIVE_WIRE {
                continue;
            }
            let frame = decode_frame(&notification.value[1..]).map_err(embedded_to_io)?;
            match frame.kind {
                FRAME_KIND_ANNOUNCE => {
                    log::trace!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    log::trace!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} body={} source={} destination={}",
                        frame.kind,
                        frame.sequence,
                        String::from_utf8_lossy(&envelope.body),
                        hex_lower(&envelope.source),
                        hex_lower(&envelope.destination)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::LxmfPing && envelope.body.starts_with(b"pong:") {
                        break;
                    }
                }
                _ => {
                    log::trace!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} payload_hex={}",
                        frame.kind,
                        frame.sequence,
                        hex_lower(&frame.payload)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::RawPing && frame.kind != FRAME_KIND_ANNOUNCE {
                        break;
                    }
                }
            }
        }

        let _ = peripheral.unsubscribe(&notify_char).await;
        let _ = peripheral.disconnect().await;
        log::info!(
            "BLE_NATIVE_PEER ok: device_id={} responses={} mode={:?}",
            peripheral.id(),
            responses,
            mode
        );
        Ok(())
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) fn run_ble_native_bridge(
    scan_secs: u64,
    name_hint: String,
    peripheral_id: Option<String>,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
    rpc: String,
    runtime_seq: Option<u32>,
    payload: String,
    destination_hex: String,
    source_hex: String,
    timeout_secs: u64,
    content_type: String,
) -> io::Result<()> {
    let scan_timeout = Duration::from_secs(scan_secs.max(1));
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let service_uuid = parse_gatt_uuid(service_uuid.as_str())?;
    let write_uuid = parse_gatt_uuid(write_char_uuid.as_str())?;
    let notify_uuid = parse_gatt_uuid(notify_char_uuid.as_str())?;
    let device_hint = peripheral_id.unwrap_or_else(|| name_hint.clone());
    let runtime_seq = resolve_runtime_seq(runtime_seq);
    let source = parse_hex_16(source_hex.as_str())?;
    let destination = parse_hex_16(destination_hex.as_str())?;
    let envelope = MinimalEnvelope {
        source,
        destination,
        sequence: u64::from(runtime_seq),
        body: payload.clone().into_bytes(),
    };
    let runtime_frame = encode_frame(
        &PacketFrame::new(
            FRAME_KIND_LXMF_MESSAGE,
            runtime_seq,
            encode_envelope(&envelope).map_err(embedded_to_io)?,
        )
        .map_err(embedded_to_io)?,
    )
    .map_err(embedded_to_io)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;
        let peripheral: Peripheral =
            find_peripheral(&adapter, device_hint.as_str(), Some(service_uuid), scan_timeout)
                .await?;

        let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
        if !connected {
            peripheral.connect().await.map_err(io::Error::other)?;
        }
        peripheral.discover_services().await.map_err(io::Error::other)?;
        let characteristics = peripheral.characteristics();
        let write_char = characteristics
            .iter()
            .find(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("write characteristic not found"))?;
        let notify_char = characteristics
            .iter()
            .find(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid)
            .cloned()
            .ok_or_else(|| io::Error::other("notify characteristic not found"))?;
        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if write_char.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            return Err(io::Error::other("write characteristic has no write capability"));
        };

        let mut notifications = peripheral.notifications().await.map_err(io::Error::other)?;
        peripheral.subscribe(&notify_char).await.map_err(io::Error::other)?;

        let mut outbound = Vec::with_capacity(1 + runtime_frame.len());
        outbound.push(BLE_FRAME_NATIVE_WIRE);
        outbound.extend_from_slice(&runtime_frame);
        peripheral
            .write(&write_char, outbound.as_slice(), write_type)
            .await
            .map_err(io::Error::other)?;

        let deadline = Instant::now() + timeout;
        let mut attachment_id: Option<String> = None;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let notification = match tokio::time::timeout(remaining, notifications.next()).await {
                Ok(Some(notification)) => notification,
                Ok(None) => break,
                Err(_) => break,
            };
            if notification.uuid != notify_uuid || notification.value.is_empty() {
                continue;
            }
            if notification.value[0] != BLE_FRAME_NATIVE_WIRE {
                continue;
            }
            let frame = decode_frame(&notification.value[1..]).map_err(embedded_to_io)?;
            if frame.kind != FRAME_KIND_LXMF_MESSAGE {
                continue;
            }
            let reply = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
            let attachment = upload_attachment_via_rpc(
                rpc.as_str(),
                "ble-native-bridge.txt".to_string(),
                content_type.clone(),
                reply.body.as_slice(),
                4096,
            )?;
            log::info!(
                "BLE_NATIVE_BRIDGE ok: device_id={} frame_seq={} body={} attachment_id={}",
                peripheral.id(),
                frame.sequence,
                String::from_utf8_lossy(&reply.body),
                attachment
            );
            attachment_id = Some(attachment);
            break;
        }

        let _ = peripheral.unsubscribe(&notify_char).await;
        let _ = peripheral.disconnect().await;
        if attachment_id.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no LXMF response frame received before timeout",
            ));
        }
        Ok(())
    })
}
