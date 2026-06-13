use super::lora_state::ensure_state_file;
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::kiss::KissIdBeaconConfig;
use rns_transport::iface::lora::{LoraConfig, LoraInterface};
#[cfg(feature = "rnode-ble")]
use rns_transport::iface::rnode_ble::{NativeRnodeBleKissInterface, NativeRnodeBleSettings};
use rns_transport::iface::rnode_ble::{RnodeBleKissConfig, RNODE_BLE_READ_FRAME_TIMEOUT};
use std::time::Duration;

pub(crate) fn startup(iface: &InterfaceConfig) -> Result<(), String> {
    let path = iface
        .state_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "lora.state_path is required".to_string())?;

    let state = ensure_state_file(path)?;

    log::info!(
        "[daemon] lora configured name={} region={} state_path={} duty_cycle_debt_ms={} debt_elapsed_ms={} uncertain={}",
        iface.name.as_deref().unwrap_or("<unnamed>"),
        iface.region.as_deref().unwrap_or("<unset>"),
        path,
        state.duty_cycle_debt_ms,
        state.debt_elapsed_ms,
        state.uncertain
    );

    if state.duty_cycle_debt_ms > 0 {
        log::info!(
            "[daemon] lora compliance gate name={} debt_remaining_ms={} tx_allowed_after_additional_wait_ms={}",
            iface.name.as_deref().unwrap_or("<unnamed>"),
            state.duty_cycle_debt_ms,
            state.duty_cycle_debt_ms
        );
    }

    Ok(())
}

pub(crate) fn has_active_device(iface: &InterfaceConfig) -> bool {
    iface.device.as_deref().map(str::trim).is_some_and(|value| !value.is_empty())
}

pub(crate) fn is_tcp_rnode_port(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("tcp://")
}

pub(crate) fn is_ble_rnode_port(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("ble://")
}

#[derive(Debug, Clone)]
pub(crate) struct RnodeBleDaemonConfig {
    pub(crate) peripheral_id: String,
    pub(crate) adapter: Option<String>,
    pub(crate) lora: LoraConfig,
    pub(crate) transport: RnodeBleKissConfig,
    pub(crate) startup_response_timeout: Duration,
    pub(crate) reconnect_backoff: Duration,
    pub(crate) max_reconnect_backoff: Duration,
    #[cfg_attr(not(feature = "rnode-ble"), allow(dead_code))]
    pub(crate) detection_fallback_timeout: Option<Duration>,
}

pub(crate) fn build_rnode_ble_config(
    iface: &InterfaceConfig,
) -> Result<RnodeBleDaemonConfig, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "lora.device is required for RNodeInterface ble://".to_string())?;
    if !is_ble_rnode_port(device) {
        return Err("RNodeInterface BLE device must start with ble://".to_string());
    }
    let peripheral_id = device
        .get("ble://".len()..)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "RNodeInterface ble:// port must include a peripheral id".to_string())?
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
    let lora_config = build_lora_config(iface)?;

    Ok(RnodeBleDaemonConfig {
        peripheral_id,
        adapter,
        lora: lora_config,
        startup_response_timeout: Duration::from_millis(iface.connect_timeout_ms.unwrap_or(5_000)), // was 1_500; matches Python's ble_detect_timeout
        transport: RnodeBleKissConfig {
            scan_timeout: Duration::from_millis(iface.scan_timeout_ms.unwrap_or(2_000)),
            connect_timeout: Duration::from_millis(iface.ble_connect_timeout_ms.unwrap_or(5_000)),
            read_frame_timeout: RNODE_BLE_READ_FRAME_TIMEOUT,
            mtu: usize::from(lora_config.max_payload_bytes),
            max_write_len: iface.max_write_len.unwrap_or(20),
            write_with_response: false,
            initial_frames: lora_config.probe_frames(),
            deferred_frames: lora_config.radio_config_frames(),
            shutdown_frames: lora_config.shutdown_frames(),
            kiss: rnode_kiss_config(iface),
            ..RnodeBleKissConfig::default()
        },
        reconnect_backoff: Duration::from_millis(reconnect_backoff_ms),
        max_reconnect_backoff: Duration::from_millis(max_reconnect_backoff_ms),
        detection_fallback_timeout: iface
            .detection_fallback_timeout_ms
            .map(|ms| Duration::from_millis(ms.max(100))),
    })
}

#[cfg(feature = "rnode-ble")]
pub(crate) fn build_native_rnode_ble_interface(
    iface: &InterfaceConfig,
    config: RnodeBleDaemonConfig,
) -> NativeRnodeBleKissInterface {
    let mut settings = NativeRnodeBleSettings::for_peripheral(config.peripheral_id.clone());
    settings.scan_timeout = config.transport.scan_timeout;
    settings.connect_timeout = config.transport.connect_timeout;
    settings.notification_timeout = config.transport.read_frame_timeout;
    if let Some(adapter) = config.adapter.as_deref() {
        settings = settings.with_adapter(adapter.to_string());
    }

    let mut iface = NativeRnodeBleKissInterface::new(
        iface.name.clone().unwrap_or_else(|| "<unnamed>".to_string()),
        settings,
        config.transport,
    )
    .with_rnode_validation(config.lora, config.startup_response_timeout)
    .with_reconnect_backoff(config.reconnect_backoff)
    .with_max_reconnect_backoff(config.max_reconnect_backoff);
    if let Some(timeout) = config.detection_fallback_timeout {
        iface = iface.with_detection_fallback_timeout(timeout);
    }
    iface
}

pub(crate) fn build_adapter(iface: &InterfaceConfig) -> Result<LoraInterface, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "lora.device is required".to_string())?;
    let config = build_lora_config(iface)?;

    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms = iface
        .max_reconnect_backoff_ms
        .unwrap_or_else(|| reconnect_backoff_ms.max(5_000))
        .max(reconnect_backoff_ms);
    let startup_response_timeout_ms = iface.connect_timeout_ms.unwrap_or(1_500);

    let kiss = rnode_kiss_config(iface);
    let adapter = if is_tcp_rnode_port(device) {
        let addr = device
            .trim()
            .strip_prefix("tcp://")
            .or_else(|| device.trim().strip_prefix("TCP://"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "lora tcp port must include an address after tcp://".to_string())?;
        LoraInterface::new_tcp(addr.to_string(), config)
    } else {
        let baud_rate = iface.baud_rate.ok_or_else(|| "lora.baud_rate is required".to_string())?;
        if baud_rate == 0 {
            return Err("lora.baud_rate must be > 0".to_string());
        }
        LoraInterface::new(device.to_string(), baud_rate, config)
    };

    Ok(adapter
        .with_flow_control(kiss.flow_control)
        .with_id_beacon(kiss.id_beacon)
        .with_reconnect_backoff(Duration::from_millis(reconnect_backoff_ms))
        .with_max_reconnect_backoff(Duration::from_millis(max_reconnect_backoff_ms))
        .with_startup_response_timeout(Duration::from_millis(startup_response_timeout_ms)))
}

fn build_lora_config(iface: &InterfaceConfig) -> Result<LoraConfig, String> {
    let region = iface
        .region
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "lora.region is required".to_string())?;
    let mut config = LoraConfig::for_region(region)
        .ok_or_else(|| format!("unsupported lora.region {region}"))?;

    if let Some(frequency_hz) = iface.frequency_hz {
        config.frequency_hz = frequency_hz;
    }
    if let Some(bandwidth_hz) = iface.bandwidth_hz {
        config.bandwidth_hz = bandwidth_hz;
    }
    if let Some(spreading_factor) = iface.spreading_factor {
        config.spreading_factor = spreading_factor;
    }
    if let Some(coding_rate) = iface.coding_rate.as_deref() {
        config.coding_rate = parse_coding_rate(coding_rate)?;
    }
    if let Some(tx_power_dbm) = iface.tx_power_dbm {
        config.tx_power_dbm = tx_power_dbm;
    }
    if let Some(limit) = iface.airtime_limit_short {
        config.airtime_limit_short_hundredths =
            Some(airtime_limit_hundredths("lora.airtime_limit_short", limit)?);
    }
    if let Some(limit) = iface.airtime_limit_long {
        config.airtime_limit_long_hundredths =
            Some(airtime_limit_hundredths("lora.airtime_limit_long", limit)?);
    }
    if let Some(max_payload_bytes) = iface.max_payload_bytes {
        config.max_payload_bytes = max_payload_bytes;
    }
    config.validate()?;
    Ok(config)
}

fn rnode_kiss_config(iface: &InterfaceConfig) -> rns_transport::iface::kiss::KissConfig {
    rns_transport::iface::kiss::KissConfig {
        preamble_ms: iface.preamble_ms.unwrap_or(350),
        tx_tail_ms: iface.tx_tail_ms.unwrap_or(20),
        persistence: iface.persistence.unwrap_or(64),
        slot_time_ms: iface.slot_time_ms.unwrap_or(20),
        flow_control: iface.flow_control.as_ref().and_then(toml::Value::as_bool).unwrap_or(false),
        id_beacon: iface.id_callsign.as_deref().zip(iface.id_interval).map(
            |(callsign, interval)| KissIdBeaconConfig {
                callsign: callsign.as_bytes().to_vec(),
                interval: Duration::from_secs(interval),
                min_payload_len: 0,
            },
        ),
    }
}

fn parse_coding_rate(value: &str) -> Result<u8, String> {
    match value.trim() {
        "4/5" | "5" => Ok(5),
        "4/6" | "6" => Ok(6),
        "4/7" | "7" => Ok(7),
        "4/8" | "8" => Ok(8),
        _ => Err(format!("lora.coding_rate must be one of 4/5, 4/6, 4/7, 4/8 (got {value})")),
    }
}

fn airtime_limit_hundredths(field: &str, value: f64) -> Result<u16, String> {
    if !(0.0..=100.0).contains(&value) {
        return Err(format!("{field} must be between 0 and 100"));
    }
    Ok((value * 100.0).trunc() as u16)
}
