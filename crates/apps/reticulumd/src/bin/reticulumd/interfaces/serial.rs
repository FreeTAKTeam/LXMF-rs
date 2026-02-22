use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::serial::SerialInterface;
use std::time::Duration;

pub(crate) fn build_adapter(iface: &InterfaceConfig) -> Result<SerialInterface, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "serial.device is required".to_string())?;
    let baud_rate = iface.baud_rate.ok_or_else(|| "serial.baud_rate is required".to_string())?;

    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms = iface
        .max_reconnect_backoff_ms
        .unwrap_or_else(|| reconnect_backoff_ms.max(5_000))
        .max(reconnect_backoff_ms);
    let mtu = iface.mtu.unwrap_or(2048);

    let mut adapter = SerialInterface::new(device.to_string(), baud_rate)
        .with_mtu(mtu)
        .with_reconnect_backoff(Duration::from_millis(reconnect_backoff_ms))
        .with_max_reconnect_backoff(Duration::from_millis(max_reconnect_backoff_ms));

    if let Some(data_bits) = iface.data_bits {
        adapter = adapter.with_data_bits_raw(data_bits)?;
    }
    if let Some(stop_bits) = iface.stop_bits {
        adapter = adapter.with_stop_bits_raw(stop_bits)?;
    }
    if let Some(parity) = iface.parity.as_deref() {
        adapter = adapter.with_parity_name(parity)?;
    }
    if let Some(flow_control) = iface.flow_control.as_deref() {
        adapter = adapter.with_flow_control_name(flow_control)?;
    }

    Ok(adapter)
}
