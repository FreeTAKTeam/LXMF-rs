#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
))]
use btleplug::api::{Central, Peripheral as _};
#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
))]
use btleplug::platform::Peripheral;
#[cfg(any(
    target_os = "android",
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
))]
use uuid::Uuid;

use std::io;
use std::time::{Duration, Instant};

use crate::hex_lower;

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) fn format_manufacturer_data(data: &std::collections::HashMap<u16, Vec<u8>>) -> String {
    let mut entries: Vec<String> = data
        .iter()
        .map(|(company, payload)| {
            format!("{company:#06x}:{}:{}", hex_lower(payload), ascii_safe(payload))
        })
        .collect();
    entries.sort_unstable();
    format!("[{}]", entries.join(","))
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
fn ascii_safe(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| if byte.is_ascii_graphic() || *byte == b' ' { *byte as char } else { '.' })
        .collect()
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) fn ascii_lower(bytes: &[u8]) -> String {
    ascii_safe(bytes).to_lowercase()
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) async fn find_peripheral(
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

#[cfg(any(target_os = "android", target_os = "linux", target_os = "windows"))]
pub(super) async fn find_camera_peripheral_by_profile(
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

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
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

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
fn identifiers_match(configured: &str, discovered: &str) -> bool {
    let configured = normalize_identifier(configured);
    let discovered = normalize_identifier(discovered);
    configured == discovered || discovered.contains(&configured) || configured.contains(&discovered)
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) fn normalize_identifier(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, ':' | '-'))
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(super) fn parse_gatt_uuid(value: &str) -> io::Result<Uuid> {
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
