use reticulum_daemon::config::InterfaceConfig;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone)]
pub(crate) struct BleRuntimeSettings {
    pub(crate) adapter: Option<String>,
    pub(crate) peripheral_id: String,
    pub(crate) service_uuid: String,
    pub(crate) write_char_uuid: String,
    pub(crate) notify_char_uuid: String,
    pub(crate) mtu: usize,
    pub(crate) scan_timeout: Duration,
    pub(crate) connect_timeout: Duration,
    pub(crate) reconnect_backoff: Duration,
    pub(crate) max_reconnect_backoff: Duration,
}

pub(crate) fn startup(iface: &InterfaceConfig) -> Result<(), String> {
    let settings = runtime_settings(iface)?;

    #[cfg(target_os = "linux")]
    {
        return linux::startup(iface, &settings);
    }
    #[cfg(target_os = "macos")]
    {
        return macos::startup(iface, &settings);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::startup(iface, &settings);
    }
    #[allow(unreachable_code)]
    Err(format!(
        "ble_gatt is not available on this target for interface {}",
        iface.name.as_deref().unwrap_or("<unnamed>")
    ))
}

fn runtime_settings(iface: &InterfaceConfig) -> Result<BleRuntimeSettings, String> {
    let peripheral_id = required_non_empty(iface.peripheral_id.as_deref(), "peripheral_id")?;
    let service_uuid = required_non_empty(iface.service_uuid.as_deref(), "service_uuid")?;
    let write_char_uuid = required_non_empty(iface.write_char_uuid.as_deref(), "write_char_uuid")?;
    let notify_char_uuid =
        required_non_empty(iface.notify_char_uuid.as_deref(), "notify_char_uuid")?;

    let adapter = iface
        .adapter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let mtu = iface.mtu.unwrap_or(247).clamp(23, 517);
    let scan_timeout_ms = iface.scan_timeout_ms.unwrap_or(5_000);
    let connect_timeout_ms = iface.connect_timeout_ms.unwrap_or(10_000);
    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms =
        iface.max_reconnect_backoff_ms.unwrap_or_else(|| reconnect_backoff_ms.max(5_000));
    if max_reconnect_backoff_ms < reconnect_backoff_ms {
        return Err("ble_gatt.max_reconnect_backoff_ms must be >= ble_gatt.reconnect_backoff_ms"
            .to_string());
    }

    Ok(BleRuntimeSettings {
        adapter,
        peripheral_id,
        service_uuid,
        write_char_uuid,
        notify_char_uuid,
        mtu,
        scan_timeout: Duration::from_millis(scan_timeout_ms),
        connect_timeout: Duration::from_millis(connect_timeout_ms),
        reconnect_backoff: Duration::from_millis(reconnect_backoff_ms),
        max_reconnect_backoff: Duration::from_millis(max_reconnect_backoff_ms),
    })
}

fn required_non_empty(value: Option<&str>, field: &str) -> Result<String, String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("ble_gatt.{field} is required"))
}

#[cfg(test)]
mod tests {
    use super::runtime_settings;
    use reticulum_daemon::config::InterfaceConfig;

    fn ble_iface() -> InterfaceConfig {
        InterfaceConfig {
            kind: "ble_gatt".to_string(),
            enabled: Some(true),
            peripheral_id: Some("AA:BB:CC:DD:EE:FF".to_string()),
            service_uuid: Some("12345678-1234-1234-1234-1234567890ab".to_string()),
            write_char_uuid: Some("2A37".to_string()),
            notify_char_uuid: Some("2A38".to_string()),
            ..InterfaceConfig::default()
        }
    }

    #[test]
    fn runtime_settings_use_safe_defaults() {
        let iface = ble_iface();
        let settings = runtime_settings(&iface).expect("runtime settings");
        assert_eq!(settings.mtu, 247);
        assert_eq!(settings.scan_timeout.as_millis(), 5_000);
        assert_eq!(settings.connect_timeout.as_millis(), 10_000);
        assert_eq!(settings.reconnect_backoff.as_millis(), 500);
        assert_eq!(settings.max_reconnect_backoff.as_millis(), 5_000);
    }

    #[test]
    fn runtime_settings_rejects_max_backoff_below_base() {
        let mut iface = ble_iface();
        iface.reconnect_backoff_ms = Some(5_000);
        iface.max_reconnect_backoff_ms = Some(100);
        let err = runtime_settings(&iface).expect_err("backoff bounds should fail");
        assert!(err.contains("max_reconnect_backoff_ms"));
    }
}
