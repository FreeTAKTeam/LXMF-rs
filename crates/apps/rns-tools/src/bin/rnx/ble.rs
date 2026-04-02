#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::api::{Central, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use btleplug::platform::{Manager, Peripheral};
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use futures::StreamExt;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use uuid::Uuid;

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
use rns_rpc::e2e_harness::timestamp_millis;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) fn run_camera_capture_upload(
    rpc: String,
    peripheral_id: String,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
    content_type: String,
    chunk_size: usize,
    timeout_secs: u64,
) -> io::Result<()> {
    if chunk_size == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "--chunk-size must be > 0"));
    }
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let bytes = capture_camera_over_ble(
        peripheral_id.as_str(),
        service_uuid.as_str(),
        write_char_uuid.as_str(),
        notify_char_uuid.as_str(),
        timeout,
    )?;
    if bytes.is_empty() {
        return Err(io::Error::other("camera capture returned empty payload"));
    }

    let name = format!("capture-{}.jpg", timestamp_millis());
    let attachment_id =
        upload_attachment_via_rpc(rpc.as_str(), name, content_type, bytes.as_slice(), chunk_size)?;
    println!("CAMERA_CAPTURE_UPLOAD ok: bytes={} attachment_id={}", bytes.len(), attachment_id);
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn run_camera_capture_upload(
    _rpc: String,
    _peripheral_id: String,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _content_type: String,
    _chunk_size: usize,
    _timeout_secs: u64,
) -> io::Result<()> {
    Err(io::Error::other("camera-capture-upload is only supported on linux/macos/windows"))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn capture_camera_over_ble(
    peripheral_id: &str,
    service_uuid: &str,
    write_char_uuid: &str,
    notify_char_uuid: &str,
    timeout: Duration,
) -> io::Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async {
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;

        let service_uuid = parse_gatt_uuid(service_uuid)?;
        let write_uuid = parse_gatt_uuid(write_char_uuid)?;
        let notify_uuid = parse_gatt_uuid(notify_char_uuid)?;
        let peripheral =
            match find_peripheral(&adapter, peripheral_id, Some(service_uuid), timeout).await {
                Ok(peripheral) => peripheral,
                Err(error) => {
                    #[cfg(target_os = "macos")]
                    {
                        return Err(io::Error::other(format!(
                            "{error}; on macOS use `rnx ble-find-camera` first and pass the returned id"
                        )));
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = &error;
                        find_camera_peripheral_by_profile(
                            &adapter,
                            peripheral_id,
                            service_uuid,
                            write_uuid,
                            notify_uuid,
                            timeout,
                        )
                        .await?
                    }
                }
            };
        if !peripheral.is_connected().await.map_err(io::Error::other)? {
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

        const FRAME_CAPTURE_REQ: u8 = 0x02;
        const FRAME_CAPTURE_ACK: u8 = 0x03;
        const FRAME_CHUNK: u8 = 0x04;
        const FRAME_CHUNK_ACK: u8 = 0x05;
        const FRAME_DONE: u8 = 0x06;
        const FRAME_ERROR: u8 = 0x07;
        const FRAME_NACK: u8 = 0x08;

        peripheral
            .write(&write_char, &[FRAME_CAPTURE_REQ], write_type)
            .await
            .map_err(io::Error::other)?;

        let deadline = Instant::now() + timeout;
        let mut transfer_id: Option<u32> = None;
        let mut expected_seq: u16 = 0;
        let mut bytes = Vec::new();

        loop {
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "camera capture timed out"));
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            let notification = tokio::time::timeout(remaining, notifications.next())
                .await
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::TimedOut, "capture notification timeout")
                })?
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "notification stream closed")
                })?;

            if notification.uuid != notify_uuid || notification.value.is_empty() {
                continue;
            }

            let frame = notification.value;
            match frame[0] {
                FRAME_CAPTURE_ACK => continue,
                FRAME_ERROR => {
                    let message = String::from_utf8_lossy(&frame[1..]).to_string();
                    return Err(io::Error::other(format!("camera error: {message}")));
                }
                FRAME_DONE => break,
                FRAME_CHUNK => {
                    if frame.len() < 15 {
                        continue;
                    }
                    let fid = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                    let seq = u16::from_le_bytes([frame[5], frame[6]]);
                    let _total = u16::from_le_bytes([frame[7], frame[8]]);
                    let payload_len = u16::from_le_bytes([frame[9], frame[10]]) as usize;
                    let _crc32 = u32::from_le_bytes([frame[11], frame[12], frame[13], frame[14]]);
                    let payload = &frame[15..];
                    if payload.len() != payload_len {
                        continue;
                    }
                    if transfer_id.is_none() {
                        transfer_id = Some(fid);
                    }
                    if transfer_id != Some(fid) {
                        continue;
                    }
                    if seq == expected_seq {
                        bytes.extend_from_slice(payload);
                        let mut ack = Vec::with_capacity(7);
                        ack.push(FRAME_CHUNK_ACK);
                        ack.extend_from_slice(&fid.to_le_bytes());
                        ack.extend_from_slice(&seq.to_le_bytes());
                        peripheral
                            .write(&write_char, &ack, write_type)
                            .await
                            .map_err(io::Error::other)?;
                        expected_seq = expected_seq.saturating_add(1);
                    } else if seq < expected_seq {
                        let mut ack = Vec::with_capacity(7);
                        ack.push(FRAME_CHUNK_ACK);
                        ack.extend_from_slice(&fid.to_le_bytes());
                        ack.extend_from_slice(&seq.to_le_bytes());
                        peripheral
                            .write(&write_char, &ack, write_type)
                            .await
                            .map_err(io::Error::other)?;
                    } else {
                        let mut nack = Vec::with_capacity(7);
                        nack.push(FRAME_NACK);
                        nack.extend_from_slice(&fid.to_le_bytes());
                        nack.extend_from_slice(&expected_seq.to_le_bytes());
                        peripheral
                            .write(&write_char, &nack, write_type)
                            .await
                            .map_err(io::Error::other)?;
                    }
                }
                _ => {}
            }
        }

        let _ = peripheral.unsubscribe(&notify_char).await;
        let _ = peripheral.disconnect().await;
        Ok(bytes)
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) fn run_ble_scan(
    timeout_secs: u64,
    limit: usize,
    service_uuid: Option<String>,
    manufacturer_prefix: Option<String>,
) -> io::Result<()> {
    let timeout = Duration::from_secs(timeout_secs.max(1));
    let service_filter = match service_uuid {
        Some(value) => Some(parse_gatt_uuid(value.as_str())?),
        None => None,
    };
    let manufacturer_filter = manufacturer_prefix.as_deref().map(normalize_identifier);
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
        tokio::time::sleep(timeout).await;

        let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
        let mut shown = 0usize;
        for peripheral in peripherals {
            let id = peripheral.id().to_string();
            let properties = peripheral.properties().await.map_err(io::Error::other)?;
            let Some(properties) = properties else {
                continue;
            };
            if let Some(expected_service) = service_filter {
                if !properties.services.contains(&expected_service) {
                    continue;
                }
            }
            if let Some(expected_marker) = manufacturer_filter.as_deref() {
                let mut marker_match = false;
                for payload in properties.manufacturer_data.values() {
                    let hex = hex_lower(payload);
                    let ascii = ascii_lower(payload);
                    if hex.contains(expected_marker) || ascii.contains(expected_marker) {
                        marker_match = true;
                        break;
                    }
                }
                if !marker_match {
                    continue;
                }
            }
            let manufacturer = format_manufacturer_data(&properties.manufacturer_data);
            println!(
                "BLE_SCAN device id={} name={} address={} rssi={:?} services={:?} manufacturer={}",
                id,
                properties.local_name.as_deref().unwrap_or("<none>"),
                properties.address,
                properties.rssi,
                properties.services,
                manufacturer
            );
            shown = shown.saturating_add(1);
            if limit > 0 && shown >= limit {
                break;
            }
        }
        println!("BLE_SCAN done: devices_shown={shown}");
        Ok(())
    })
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) fn run_ble_find_camera(
    scan_secs: u64,
    name_hint: String,
    service_uuid: String,
    write_char_uuid: String,
    notify_char_uuid: String,
) -> io::Result<()> {
    let scan_timeout = Duration::from_secs(scan_secs.max(1));
    let service_uuid = parse_gatt_uuid(service_uuid.as_str())?;
    let write_uuid = parse_gatt_uuid(write_char_uuid.as_str())?;
    let notify_uuid = parse_gatt_uuid(notify_char_uuid.as_str())?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;
    runtime.block_on(async move {
        let hint_norm = normalize_identifier(name_hint.as_str());
        let deadline = Instant::now() + scan_timeout;
        let manager = Manager::new().await.map_err(io::Error::other)?;
        let adapters = manager.adapters().await.map_err(io::Error::other)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| io::Error::other("no BLE adapter available"))?;
        adapter.start_scan(ScanFilter::default()).await.map_err(io::Error::other)?;

        while Instant::now() < deadline {
            let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
            let mut candidates: Vec<(u8, i16, Peripheral, String)> =
                Vec::with_capacity(peripherals.len());
            for peripheral in peripherals {
                let mut rank = 3_u8;
                let mut rssi = -127_i16;
                let mut name = "<none>".to_string();
                if let Some(props) = peripheral.properties().await.map_err(io::Error::other)? {
                    rssi = props.rssi.unwrap_or(-127);
                    if let Some(local_name) = props.local_name {
                        name = local_name;
                        let norm = normalize_identifier(name.as_str());
                        if !hint_norm.is_empty() && norm.contains(&hint_norm) {
                            rank = 0;
                        } else if norm.contains("lxmf") {
                            rank = 1;
                        } else if norm != "<none>" {
                            rank = 2;
                        }
                    }
                }
                candidates.push((rank, rssi, peripheral, name));
            }
            candidates.sort_by(|(rank_a, rssi_a, _, _), (rank_b, rssi_b, _, _)| {
                rank_a.cmp(rank_b).then(rssi_b.cmp(rssi_a))
            });

            for (_, rssi, peripheral, name) in candidates {
                let id = peripheral.id().to_string();
                let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
                if !connected
                    && tokio::time::timeout(Duration::from_millis(650), peripheral.connect())
                        .await
                        .is_err()
                {
                    continue;
                }
                if peripheral.discover_services().await.is_err() {
                    let _ = peripheral.disconnect().await;
                    continue;
                }
                let chars = peripheral.characteristics();
                let has_write =
                    chars.iter().any(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid);
                let has_notify = chars
                    .iter()
                    .any(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid);

                if has_write && has_notify {
                    println!("BLE_FIND_CAMERA match id={} name={} rssi={}", id, name, rssi);
                    let _ = peripheral.disconnect().await;
                    return Ok(());
                }
                let _ = peripheral.disconnect().await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no scanned peripheral matched requested camera profile",
        ))
    })
}

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
                    println!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} bytes={} role=announce",
                        frame.kind,
                        frame.sequence,
                        frame.payload.len()
                    );
                }
                FRAME_KIND_LXMF_MESSAGE => {
                    let envelope = decode_envelope(&frame.payload).map_err(embedded_to_io)?;
                    println!(
                        "BLE_NATIVE_PEER frame kind=0x{:02x} seq={} body={} source={} destination={}",
                        frame.kind,
                        frame.sequence,
                        String::from_utf8_lossy(&envelope.body),
                        hex_lower(&envelope.source),
                        hex_lower(&envelope.destination)
                    );
                    responses = responses.saturating_add(1);
                    if mode == NativePeerMode::LxmfPing
                        && envelope.body.starts_with(b"pong:")
                    {
                        break;
                    }
                }
                _ => {
                    println!(
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
        println!(
            "BLE_NATIVE_PEER ok: device_id={} responses={} mode={:?}",
            peripheral.id(),
            responses,
            mode
        );
        Ok(())
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn run_ble_native_peer(
    _scan_secs: u64,
    _name_hint: String,
    _peripheral_id: Option<String>,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _mode: NativePeerMode,
    _runtime_seq: Option<u32>,
    _payload: String,
    _destination_hex: String,
    _source_hex: String,
    _timeout_secs: u64,
) -> io::Result<()> {
    Err(io::Error::other("ble-native-peer is only supported on linux/macos/windows"))
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
            println!(
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

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn run_ble_native_bridge(
    _scan_secs: u64,
    _name_hint: String,
    _peripheral_id: Option<String>,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
    _rpc: String,
    _runtime_seq: Option<u32>,
    _payload: String,
    _destination_hex: String,
    _source_hex: String,
    _timeout_secs: u64,
    _content_type: String,
) -> io::Result<()> {
    Err(io::Error::other("ble-native-bridge is only supported on linux/macos/windows"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn run_ble_find_camera(
    _scan_secs: u64,
    _name_hint: String,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
) -> io::Result<()> {
    Err(io::Error::other("ble-find-camera is only supported on linux/macos/windows"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) fn run_ble_scan(
    _timeout_secs: u64,
    _limit: usize,
    _service_uuid: Option<String>,
    _manufacturer_prefix: Option<String>,
) -> io::Result<()> {
    Err(io::Error::other("ble-scan is only supported on linux/macos/windows"))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn format_manufacturer_data(data: &std::collections::HashMap<u16, Vec<u8>>) -> String {
    let mut entries: Vec<String> = data
        .iter()
        .map(|(company, payload)| {
            format!("{company:#06x}:{}:{}", hex_lower(payload), ascii_safe(payload))
        })
        .collect();
    entries.sort_unstable();
    format!("[{}]", entries.join(","))
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn ascii_safe(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' })
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn ascii_lower(bytes: &[u8]) -> String {
    ascii_safe(bytes).to_lowercase()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn find_peripheral(
    adapter: &btleplug::platform::Adapter,
    peripheral_id: &str,
    service_uuid: Option<Uuid>,
    timeout: Duration,
) -> io::Result<Peripheral> {
    let deadline = Instant::now() + timeout;
    loop {
        let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
        for peripheral in peripherals {
            if peripheral_matches(&peripheral, peripheral_id, service_uuid).await? {
                return Ok(peripheral);
            }
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "BLE peripheral not found"));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
async fn find_camera_peripheral_by_profile(
    adapter: &btleplug::platform::Adapter,
    peripheral_hint: &str,
    service_uuid: Uuid,
    write_uuid: Uuid,
    notify_uuid: Uuid,
    timeout: Duration,
) -> io::Result<Peripheral> {
    let deadline = Instant::now() + timeout;
    let hint_norm = normalize_identifier(peripheral_hint);
    loop {
        let peripherals = adapter.peripherals().await.map_err(io::Error::other)?;
        let mut candidates: Vec<(u8, Peripheral)> = Vec::with_capacity(peripherals.len());
        for peripheral in peripherals {
            let mut rank = 3_u8;
            if let Ok(Some(props)) = peripheral.properties().await.map_err(io::Error::other) {
                if let Some(name) = props.local_name {
                    let name_norm = normalize_identifier(name.as_str());
                    if !hint_norm.is_empty() && name_norm.contains(&hint_norm) {
                        rank = 0;
                    } else if name_norm.contains("lxmfcamstub") || name_norm.contains("lxmf") {
                        rank = 1;
                    }
                }
            }
            candidates.push((rank, peripheral));
        }
        candidates.sort_by_key(|(rank, _)| *rank);
        for (_, peripheral) in candidates {
            let connected = peripheral.is_connected().await.map_err(io::Error::other)?;
            if !connected
                && tokio::time::timeout(Duration::from_millis(700), peripheral.connect())
                    .await
                    .is_err()
            {
                continue;
            }
            if peripheral.discover_services().await.is_err() {
                let _ = peripheral.disconnect().await;
                continue;
            }
            let chars = peripheral.characteristics();
            let has_write =
                chars.iter().any(|ch| ch.uuid == write_uuid && ch.service_uuid == service_uuid);
            let has_notify =
                chars.iter().any(|ch| ch.uuid == notify_uuid && ch.service_uuid == service_uuid);
            if has_write && has_notify {
                return Ok(peripheral);
            }
            let _ = peripheral.disconnect().await;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "BLE peripheral not found"));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn peripheral_matches(
    peripheral: &Peripheral,
    configured_id: &str,
    service_uuid: Option<Uuid>,
) -> io::Result<bool> {
    if identifiers_match(configured_id, &peripheral.id().to_string()) {
        return Ok(true);
    }
    let properties = peripheral.properties().await.map_err(io::Error::other)?;
    if let Some(properties) = properties {
        if identifiers_match(configured_id, &properties.address.to_string()) {
            return Ok(true);
        }
        if let Some(local_name) = properties.local_name {
            if identifiers_match(configured_id, &local_name) {
                return Ok(true);
            }
        }
        if let Some(expected_service) = service_uuid {
            if properties.services.into_iter().any(|service| service == expected_service) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn identifiers_match(configured: &str, discovered: &str) -> bool {
    let configured = normalize_identifier(configured);
    let discovered = normalize_identifier(discovered);
    configured == discovered || discovered.contains(&configured) || configured.contains(&discovered)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn normalize_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn parse_gatt_uuid(value: &str) -> io::Result<Uuid> {
    let normalized = value.trim();
    if normalized.len() == 4 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("0000{normalized}-0000-1000-8000-00805f9b34fb"))
            .map_err(io::Error::other);
    }
    if normalized.len() == 8 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Uuid::parse_str(&format!("{normalized}-0000-1000-8000-00805f9b34fb"))
            .map_err(io::Error::other);
    }
    Uuid::parse_str(normalized).map_err(io::Error::other)
}
