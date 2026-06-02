use super::kiss::python_kiss_id_beacon;
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::kiss::KissConfig;
#[cfg(feature = "vrn76-kiss-ble")]
use rns_transport::iface::vrn76_kiss_ble::{NativeVrn76BleSettings, NativeVrn76KissBleInterface};
use rns_transport::iface::vrn76_kiss_ble::{
    Vrn76FrameMode, Vrn76KissBleConfig, VRN76_KISS_READ_FRAME_TIMEOUT,
};
use std::time::Duration;

#[derive(Debug, Clone)]
pub(crate) struct Vrn76KissBleDaemonConfig {
    pub(crate) peripheral_id: String,
    pub(crate) adapter: Option<String>,
    pub(crate) transport: Vrn76KissBleConfig,
    pub(crate) reconnect_backoff: Duration,
    pub(crate) max_reconnect_backoff: Duration,
}

pub(crate) fn build_config(iface: &InterfaceConfig) -> Result<Vrn76KissBleDaemonConfig, String> {
    let peripheral_id = iface
        .peripheral_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "vrn76_kiss_ble.peripheral_id is required".to_string())?
        .to_string();
    let adapter = iface
        .adapter
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms = iface
        .max_reconnect_backoff_ms
        .unwrap_or_else(|| reconnect_backoff_ms.max(5_000))
        .max(reconnect_backoff_ms);

    Ok(Vrn76KissBleDaemonConfig {
        peripheral_id,
        adapter,
        transport: Vrn76KissBleConfig {
            mtu: iface.mtu.unwrap_or(564),
            max_write_len: iface.max_write_len.unwrap_or(512),
            scan_timeout: Duration::from_millis(iface.scan_timeout_ms.unwrap_or(10_000)),
            command_timeout: Duration::from_millis(iface.connect_timeout_ms.unwrap_or(3_000)),
            read_frame_timeout: VRN76_KISS_READ_FRAME_TIMEOUT,
            frame_mode: vrn76_frame_mode(iface.frame_mode.as_deref())?,
            kiss: KissConfig {
                preamble_ms: iface.preamble_ms.unwrap_or(350),
                tx_tail_ms: iface.tx_tail_ms.unwrap_or(20),
                persistence: iface.persistence.unwrap_or(64),
                slot_time_ms: iface.slot_time_ms.unwrap_or(20),
                flow_control: iface.kiss_flow_control.unwrap_or(false),
                id_beacon: python_kiss_id_beacon(iface),
            },
        },
        reconnect_backoff: Duration::from_millis(reconnect_backoff_ms),
        max_reconnect_backoff: Duration::from_millis(max_reconnect_backoff_ms),
    })
}

fn vrn76_frame_mode(value: Option<&str>) -> Result<Vrn76FrameMode, String> {
    match value.map(|value| value.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("benshi_tnc_data" | "benshi") => Ok(Vrn76FrameMode::BenshiTncData),
        Some("raw_kiss" | "raw") => Ok(Vrn76FrameMode::RawKiss),
        Some(value) => Err(format!(
            "vrn76_kiss_ble.frame_mode must be one of benshi_tnc_data, benshi, raw_kiss, raw (got {value})"
        )),
    }
}

#[cfg(feature = "vrn76-kiss-ble")]
pub(crate) fn build_native_interface(
    iface: &InterfaceConfig,
    config: Vrn76KissBleDaemonConfig,
) -> NativeVrn76KissBleInterface {
    let mut settings = NativeVrn76BleSettings::for_peripheral(config.peripheral_id.clone());
    settings.scan_timeout = config.transport.scan_timeout;
    settings.connect_timeout = config.transport.command_timeout;
    settings.notification_timeout = config.transport.command_timeout;
    if let Some(adapter) = config.adapter.as_deref() {
        settings = settings.with_adapter(adapter.to_string());
    }

    NativeVrn76KissBleInterface::new(
        iface.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
        settings,
        config.transport,
    )
    .with_reconnect_backoff(config.reconnect_backoff)
    .with_max_reconnect_backoff(config.max_reconnect_backoff)
}
