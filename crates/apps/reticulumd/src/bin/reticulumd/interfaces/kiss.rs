use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::kiss::{
    KissConfig, KissIdBeaconConfig, KissInterface, KissTcpClientInterface,
};
use std::time::Duration;

pub(crate) fn build_adapter(iface: &InterfaceConfig) -> Result<KissInterface, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "kiss.device is required".to_string())?;
    let baud_rate = iface.baud_rate.ok_or_else(|| "kiss.baud_rate is required".to_string())?;
    if baud_rate == 0 {
        return Err("kiss.baud_rate must be > 0".to_string());
    }

    let common = common_kiss_settings(iface);

    let mut adapter = KissInterface::new(device.to_string(), baud_rate)
        .with_mtu(common.mtu)
        .with_kiss_config(common.kiss)
        .with_reconnect_backoff(common.reconnect_backoff)
        .with_max_reconnect_backoff(common.max_reconnect_backoff);

    if let Some(data_bits) = iface.data_bits {
        adapter = adapter.with_data_bits_raw(data_bits)?;
    }
    if let Some(stop_bits) = iface.stop_bits {
        adapter = adapter.with_stop_bits_raw(stop_bits)?;
    }
    if let Some(parity) = iface.parity.as_deref() {
        adapter = adapter.with_parity_name(parity)?;
    }

    Ok(adapter)
}

pub(crate) fn build_tcp_client_adapter(
    iface: &InterfaceConfig,
) -> Result<KissTcpClientInterface, String> {
    let host = iface
        .host
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "kiss_tcp_client.host is required".to_string())?;
    let port = iface.port.ok_or_else(|| "kiss_tcp_client.port is required".to_string())?;
    if port == 0 {
        return Err("kiss_tcp_client.port must be > 0".to_string());
    }

    let common = common_kiss_settings(iface);
    let addr = format!("{host}:{port}");
    Ok(KissTcpClientInterface::new(addr)
        .with_mtu(common.mtu)
        .with_kiss_config(common.kiss)
        .with_reconnect_backoff(common.reconnect_backoff)
        .with_max_reconnect_backoff(common.max_reconnect_backoff))
}

struct CommonKissSettings {
    mtu: usize,
    reconnect_backoff: Duration,
    max_reconnect_backoff: Duration,
    kiss: KissConfig,
}

fn common_kiss_settings(iface: &InterfaceConfig) -> CommonKissSettings {
    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms = iface
        .max_reconnect_backoff_ms
        .unwrap_or_else(|| reconnect_backoff_ms.max(5_000))
        .max(reconnect_backoff_ms);
    CommonKissSettings {
        mtu: iface.mtu.unwrap_or(564),
        reconnect_backoff: Duration::from_millis(reconnect_backoff_ms),
        max_reconnect_backoff: Duration::from_millis(max_reconnect_backoff_ms),
        kiss: KissConfig {
            preamble_ms: iface.preamble_ms.unwrap_or(350),
            tx_tail_ms: iface.tx_tail_ms.unwrap_or(20),
            persistence: iface.persistence.unwrap_or(64),
            slot_time_ms: iface.slot_time_ms.unwrap_or(20),
            flow_control: iface.kiss_flow_control.unwrap_or(false),
            id_beacon: python_kiss_id_beacon(iface),
        },
    }
}

pub(crate) fn python_kiss_id_beacon(iface: &InterfaceConfig) -> Option<KissIdBeaconConfig> {
    iface.id_interval.map(|interval| KissIdBeaconConfig {
        callsign: iface.id_callsign.as_deref().unwrap_or("").as_bytes().to_vec(),
        interval: Duration::from_secs(interval),
        min_payload_len: 15,
    })
}
