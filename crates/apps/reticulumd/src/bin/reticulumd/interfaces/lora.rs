use super::lora_state::ensure_state_file;
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::kiss::KissIdBeaconConfig;
use rns_transport::iface::lora::{LoraConfig, LoraInterface};
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

pub(crate) fn build_adapter(iface: &InterfaceConfig) -> Result<LoraInterface, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "lora.device is required".to_string())?;
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

    let reconnect_backoff_ms = iface.reconnect_backoff_ms.unwrap_or(500).max(50);
    let max_reconnect_backoff_ms = iface
        .max_reconnect_backoff_ms
        .unwrap_or_else(|| reconnect_backoff_ms.max(5_000))
        .max(reconnect_backoff_ms);
    let startup_response_timeout_ms = iface.connect_timeout_ms.unwrap_or(1_500);

    let flow_control = iface.flow_control.as_ref().and_then(toml::Value::as_bool).unwrap_or(false);
    let id_beacon =
        iface.id_callsign.as_deref().zip(iface.id_interval).map(|(callsign, interval)| {
            KissIdBeaconConfig {
                callsign: callsign.as_bytes().to_vec(),
                interval: Duration::from_secs(interval),
                min_payload_len: 0,
            }
        });
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
        .with_flow_control(flow_control)
        .with_id_beacon(id_beacon)
        .with_reconnect_backoff(Duration::from_millis(reconnect_backoff_ms))
        .with_max_reconnect_backoff(Duration::from_millis(max_reconnect_backoff_ms))
        .with_startup_response_timeout(Duration::from_millis(startup_response_timeout_ms)))
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
