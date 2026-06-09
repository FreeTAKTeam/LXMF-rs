#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
))]
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
))]
use btleplug::platform::{Manager, Peripheral};
use std::io;
use std::time::{Duration, Instant};

#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
))]
use crate::helpers::{
    ascii_lower, format_manufacturer_data, normalize_identifier, parse_gatt_uuid,
};
use crate::{hex_lower, NativePeerMode};

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
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
    crate::ble_camera::run_camera_capture_upload(
        rpc,
        peripheral_id,
        service_uuid,
        write_char_uuid,
        notify_char_uuid,
        content_type,
        chunk_size,
        timeout_secs,
    )
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
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
    Err(io::Error::other("camera-capture-upload is only supported on android/linux/macos/windows"))
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
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
            log::trace!(
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
        log::trace!("BLE_SCAN done: devices_shown={shown}");
        Ok(())
    })
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
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
                    log::trace!("BLE_FIND_CAMERA match id={} name={} rssi={}", id, name, rssi);
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

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
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
    crate::ble_native::run_ble_native_peer(
        scan_secs,
        name_hint,
        peripheral_id,
        service_uuid,
        write_char_uuid,
        notify_char_uuid,
        mode,
        runtime_seq,
        payload,
        destination_hex,
        source_hex,
        timeout_secs,
    )
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
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
    Err(io::Error::other("ble-native-peer is only supported on android/linux/macos/windows"))
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
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
    crate::ble_native::run_ble_native_bridge(
        scan_secs,
        name_hint,
        peripheral_id,
        service_uuid,
        write_char_uuid,
        notify_char_uuid,
        rpc,
        runtime_seq,
        payload,
        destination_hex,
        source_hex,
        timeout_secs,
        content_type,
    )
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
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
    Err(io::Error::other("ble-native-bridge is only supported on android/linux/macos/windows"))
}
#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
pub(crate) fn run_ble_find_camera(
    _scan_secs: u64,
    _name_hint: String,
    _service_uuid: String,
    _write_char_uuid: String,
    _notify_char_uuid: String,
) -> io::Result<()> {
    Err(io::Error::other("ble-find-camera is only supported on android/linux/macos/windows"))
}

#[cfg(not(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
)))]
pub(crate) fn run_ble_scan(
    _timeout_secs: u64,
    _limit: usize,
    _service_uuid: Option<String>,
    _manufacturer_prefix: Option<String>,
) -> io::Result<()> {
    Err(io::Error::other("ble-scan is only supported on android/linux/macos/windows"))
}
