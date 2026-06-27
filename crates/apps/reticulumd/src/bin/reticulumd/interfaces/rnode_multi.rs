use super::lora;
use reticulum_daemon::config::InterfaceConfig;
use rns_transport::iface::kiss::KissIdBeaconConfig;
use rns_transport::iface::rnode_multi::{RNodeMultiInterface, RNodeMultiSubInterfaceConfig};
use rns_transport::iface::InterfaceManager;
use std::sync::Arc;
use std::time::Duration;

pub(crate) fn build_adapter(
    iface: &InterfaceConfig,
    iface_manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
) -> Result<RNodeMultiInterface, String> {
    let device = iface
        .device
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "rnode_multi.device is required".to_string())?;
    let subinterfaces = build_subinterfaces(iface)?;
    let adapter = if lora::is_tcp_rnode_port(device) {
        let addr = device
            .trim()
            .strip_prefix("tcp://")
            .or_else(|| device.trim().strip_prefix("TCP://"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                "rnode_multi tcp port must include an address after tcp://".to_string()
            })?;
        RNodeMultiInterface::new_tcp(addr.to_string(), iface_manager)
    } else {
        let baud_rate =
            iface.baud_rate.ok_or_else(|| "rnode_multi.baud_rate is required".to_string())?;
        if baud_rate == 0 {
            return Err("rnode_multi.baud_rate must be > 0".to_string());
        }
        RNodeMultiInterface::new(device.to_string(), iface_manager).with_baud_rate(baud_rate)
    };
    let adapter =
        adapter.with_subinterfaces(subinterfaces).with_id_beacon(rnode_multi_id_beacon(iface));
    Ok(if let Some(mtu) = iface.mtu { adapter.with_mtu(mtu) } else { adapter })
}

fn rnode_multi_id_beacon(iface: &InterfaceConfig) -> Option<KissIdBeaconConfig> {
    iface.id_callsign.as_deref().zip(iface.id_interval).map(|(callsign, interval)| {
        KissIdBeaconConfig {
            callsign: callsign.as_bytes().to_vec(),
            interval: Duration::from_secs(interval),
            min_payload_len: 0,
        }
    })
}

pub(crate) fn build_subinterfaces(
    iface: &InterfaceConfig,
) -> Result<Vec<RNodeMultiSubInterfaceConfig>, String> {
    let mut subinterfaces = Vec::new();
    for (name, value) in &iface.extra {
        let Some(table) = value.as_table() else {
            return Err(format!("rnode_multi.{name} must be a subinterface table"));
        };
        if !table_enabled(table)? {
            continue;
        }
        let vport = table_u8(table, "vport")?;
        let region = table_string(table, "region")
            .or_else(|| iface.region.clone())
            .unwrap_or_else(|| "US915".to_string());
        let mut child = InterfaceConfig {
            kind: "lora".to_string(),
            enabled: Some(true),
            name: Some(table_string(table, "name").unwrap_or_else(|| name.clone())),
            region: Some(region),
            state_path: Some("<rnode-multi>".to_string()),
            frequency_hz: table_u64_alias(table, "frequency_hz", "frequency"),
            bandwidth_hz: table_u64_alias(table, "bandwidth_hz", "bandwidth")
                .map(|value| {
                    u32::try_from(value)
                        .map_err(|_| format!("rnode_multi.{name}.bandwidth must fit in u32"))
                })
                .transpose()?,
            spreading_factor: table_u8_alias(table, "spreading_factor", "spreadingfactor"),
            coding_rate: table_coding_rate(table, "coding_rate", "codingrate"),
            tx_power_dbm: table_i8_alias(table, "tx_power_dbm", "txpower").or(iface.tx_power_dbm),
            airtime_limit_short: table_f64(table, "airtime_limit_short")
                .or(iface.airtime_limit_short),
            airtime_limit_long: table_f64(table, "airtime_limit_long").or(iface.airtime_limit_long),
            max_payload_bytes: table_u64_alias(table, "max_payload_bytes", "max_payload_bytes")
                .map(|value| {
                    u16::try_from(value).map_err(|_| {
                        format!("rnode_multi.{name}.max_payload_bytes must fit in u16")
                    })
                })
                .transpose()?
                .or(iface.max_payload_bytes),
            ..InterfaceConfig::default()
        };
        child.flow_control =
            table.get("flow_control").cloned().or_else(|| iface.flow_control.clone());
        let config = lora::build_rnode_multi_lora_config(&child)
            .map_err(|err| format!("rnode_multi.{name}: {err}"))?;
        subinterfaces.push(RNodeMultiSubInterfaceConfig {
            name: child.name.unwrap_or_else(|| name.clone()),
            vport,
            config,
            outgoing: table_bool(table, "outgoing", iface.outgoing())?,
        });
    }
    if subinterfaces.is_empty() {
        return Err("rnode_multi requires at least one enabled subinterface".to_string());
    }
    Ok(subinterfaces)
}

fn table_bool(table: &toml::value::Table, key: &str, default: bool) -> Result<bool, String> {
    table.get(key).map_or(Ok(default), |value| {
        value.as_bool().ok_or_else(|| format!("{key} must be a boolean"))
    })
}

fn table_enabled(table: &toml::value::Table) -> Result<bool, String> {
    if table.contains_key("interface_enabled") {
        table_bool(table, "interface_enabled", true)
    } else {
        table_bool(table, "enabled", true)
    }
}

fn table_string(table: &toml::value::Table, key: &str) -> Option<String> {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn table_u8(table: &toml::value::Table, key: &str) -> Result<u8, String> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| format!("{key} must be an integer"))
}

fn table_u64_alias(table: &toml::value::Table, primary: &str, alias: &str) -> Option<u64> {
    table
        .get(primary)
        .or_else(|| table.get(alias))
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
}

fn table_u8_alias(table: &toml::value::Table, primary: &str, alias: &str) -> Option<u8> {
    table
        .get(primary)
        .or_else(|| table.get(alias))
        .and_then(toml::Value::as_integer)
        .and_then(|value| u8::try_from(value).ok())
}

fn table_i8_alias(table: &toml::value::Table, primary: &str, alias: &str) -> Option<i8> {
    table
        .get(primary)
        .or_else(|| table.get(alias))
        .and_then(toml::Value::as_integer)
        .and_then(|value| i8::try_from(value).ok())
}

fn table_f64(table: &toml::value::Table, key: &str) -> Option<f64> {
    table.get(key).and_then(|value| match value {
        toml::Value::Float(value) => Some(*value),
        toml::Value::Integer(value) => Some(*value as f64),
        _ => None,
    })
}

fn table_coding_rate(table: &toml::value::Table, primary: &str, alias: &str) -> Option<String> {
    let value = table.get(primary).or_else(|| table.get(alias))?;
    match value {
        toml::Value::String(value) => Some(value.clone()),
        toml::Value::Integer(value) => Some(value.to_string()),
        _ => None,
    }
}
