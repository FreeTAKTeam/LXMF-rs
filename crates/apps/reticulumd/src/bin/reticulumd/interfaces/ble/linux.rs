use super::BleRuntimeSettings;
use reticulum_daemon::config::InterfaceConfig;

pub(super) fn startup(
    iface: &InterfaceConfig,
    settings: &BleRuntimeSettings,
) -> Result<(), String> {
    eprintln!(
        "[daemon] ble_gatt configured (linux backend) name={} adapter={} peripheral_id={} service_uuid={} write_char_uuid={} notify_char_uuid={} mtu={} scan_timeout_ms={} connect_timeout_ms={} reconnect_backoff_ms={} max_reconnect_backoff_ms={}",
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
        settings.max_reconnect_backoff.as_millis()
    );
    Ok(())
}
