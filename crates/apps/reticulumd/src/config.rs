use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct DaemonConfig {
    pub display_name: Option<String>,
    pub announce_capabilities: Vec<String>,
    pub interfaces: Vec<InterfaceConfig>,
}

#[derive(Debug, Deserialize)]
struct DaemonConfigRaw {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    announce_capabilities: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_interfaces")]
    interfaces: Vec<InterfaceConfig>,
}

impl<'de> Deserialize<'de> for DaemonConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = DaemonConfigRaw::deserialize(deserializer)?;
        let mut interfaces = raw.interfaces;
        for (index, iface) in interfaces.iter_mut().enumerate() {
            let original_kind = iface.kind.trim().to_string();
            iface.kind = normalize_interface_kind(iface.kind.trim());
            iface.normalize_aliases(index, original_kind.as_str()).map_err(D::Error::custom)?;
            iface.validate(index, original_kind.as_str()).map_err(D::Error::custom)?;
        }
        Ok(Self {
            display_name: raw.display_name,
            announce_capabilities: raw.announce_capabilities,
            interfaces,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InterfaceConfig {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub interface_mode: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub frame_mode: Option<String>,
    #[serde(default)]
    pub outgoing: Option<bool>,
    #[serde(default)]
    pub bitrate: Option<u64>,
    #[serde(default)]
    pub announce_cap: Option<u64>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(skip)]
    pub port: Option<u16>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub target_host: Option<String>,
    #[serde(default)]
    pub target_port: Option<u16>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub discovery_scope: Option<String>,
    #[serde(default)]
    pub discovery_port: Option<u16>,
    #[serde(default)]
    pub data_port: Option<u16>,
    #[serde(default)]
    pub multicast_address_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_list")]
    pub devices: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_string_list")]
    pub ignored_devices: Option<Vec<String>>,
    #[serde(default)]
    pub baud_rate: Option<u32>,
    #[serde(default)]
    pub data_bits: Option<u8>,
    #[serde(default)]
    pub parity: Option<String>,
    #[serde(default)]
    pub stop_bits: Option<u8>,
    #[serde(default)]
    pub flow_control: Option<toml::Value>,
    #[serde(default)]
    pub mtu: Option<usize>,
    #[serde(default)]
    pub max_write_len: Option<usize>,
    #[serde(default)]
    pub preamble_ms: Option<u16>,
    #[serde(default)]
    pub tx_tail_ms: Option<u16>,
    #[serde(default)]
    pub persistence: Option<u8>,
    #[serde(default)]
    pub slot_time_ms: Option<u16>,
    #[serde(default)]
    pub kiss_flow_control: Option<bool>,
    #[serde(default)]
    pub id_callsign: Option<String>,
    #[serde(default)]
    pub id_interval: Option<u64>,
    #[serde(default)]
    pub reconnect_backoff_ms: Option<u64>,
    #[serde(default)]
    pub max_reconnect_backoff_ms: Option<u64>,
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub peripheral_id: Option<String>,
    #[serde(default)]
    pub service_uuid: Option<String>,
    #[serde(default)]
    pub write_char_uuid: Option<String>,
    #[serde(default)]
    pub notify_char_uuid: Option<String>,
    #[serde(default)]
    pub scan_timeout_ms: Option<u64>,
    #[serde(default)]
    pub ble_connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub frequency_hz: Option<u64>,
    #[serde(default)]
    pub bandwidth_hz: Option<u32>,
    #[serde(default)]
    pub spreading_factor: Option<u8>,
    #[serde(default)]
    pub coding_rate: Option<String>,
    #[serde(default)]
    pub tx_power_dbm: Option<i8>,
    #[serde(default)]
    pub airtime_limit_short: Option<f64>,
    #[serde(default)]
    pub airtime_limit_long: Option<f64>,
    #[serde(default)]
    pub sync_word: Option<u8>,
    #[serde(default)]
    pub preamble_symbols: Option<u16>,
    #[serde(default)]
    pub max_payload_bytes: Option<u16>,
    #[serde(default)]
    pub state_path: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

impl DaemonConfig {
    pub fn from_toml(input: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(input)
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let contents = fs::read_to_string(path)?;
        Self::from_toml(&contents)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }

    pub fn enabled_tcp_clients(&self) -> Vec<&InterfaceConfig> {
        self.interfaces
            .iter()
            .filter(|iface| iface.enabled.unwrap_or(false) && iface.kind == "tcp_client")
            .collect()
    }

    pub fn tcp_client_endpoints(&self) -> Vec<(String, u16)> {
        self.enabled_tcp_clients()
            .iter()
            .filter_map(|iface| {
                let host = iface.host.as_ref()?;
                let port = iface.port?;
                Some((host.clone(), port))
            })
            .collect()
    }

    pub fn enabled_tcp_servers(&self) -> Vec<&InterfaceConfig> {
        self.interfaces
            .iter()
            .filter(|iface| iface.enabled.unwrap_or(false) && iface.kind == "tcp_server")
            .collect()
    }

    pub fn tcp_server_endpoints(&self) -> Vec<(String, u16)> {
        self.enabled_tcp_servers()
            .iter()
            .filter_map(|iface| {
                let port = iface.port?;
                let host = iface
                    .host
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("0.0.0.0")
                    .to_string();
                Some((host, port))
            })
            .collect()
    }
}

impl InterfaceConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }

    pub fn outgoing(&self) -> bool {
        self.outgoing.unwrap_or(true)
    }

    pub fn settings_json(&self) -> Option<JsonValue> {
        let mut settings = JsonMap::new();
        if self.interface_mode_raw().is_some() {
            if let Ok(mode) = self.interface_mode() {
                settings
                    .insert("interface_mode".to_string(), JsonValue::String(mode.as_str().into()));
            }
        }
        insert_opt_bool(&mut settings, "outgoing", self.outgoing);
        insert_opt_u64(&mut settings, "bitrate", self.bitrate);
        insert_opt_u64(&mut settings, "announce_cap", self.announce_cap);
        match self.kind.as_str() {
            "tcp_client" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
            }
            "udp" => {
                insert_opt_string(&mut settings, "target_host", self.target_host.as_ref());
                insert_opt_u64(&mut settings, "target_port", self.target_port.map(u64::from));
            }
            "auto" => {
                insert_opt_string(&mut settings, "group_id", self.group_id.as_ref());
                insert_opt_string(&mut settings, "discovery_scope", self.discovery_scope.as_ref());
                insert_opt_u64(&mut settings, "discovery_port", self.discovery_port.map(u64::from));
                insert_opt_u64(&mut settings, "data_port", self.data_port.map(u64::from));
                insert_opt_string(
                    &mut settings,
                    "multicast_address_type",
                    self.multicast_address_type.as_ref(),
                );
                if let Some(address) = self.auto_discovery_multicast_address() {
                    settings.insert(
                        "discovery_multicast_address".to_string(),
                        JsonValue::String(address),
                    );
                }
                insert_opt_string_array(&mut settings, "devices", self.devices.as_ref());
                insert_opt_string_array(
                    &mut settings,
                    "ignored_devices",
                    self.ignored_devices.as_ref(),
                );
            }
            "serial" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "data_bits", self.data_bits.map(u64::from));
                insert_opt_string(&mut settings, "parity", self.parity.as_ref());
                insert_opt_u64(&mut settings, "stop_bits", self.stop_bits.map(u64::from));
                if let Some(flow_control) = self.flow_control_name() {
                    settings.insert(
                        "flow_control".to_string(),
                        JsonValue::String(flow_control.to_string()),
                    );
                }
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "kiss" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "kiss_tcp_client" => {
                insert_opt_string(&mut settings, "host", self.host.as_ref());
                insert_opt_u64(&mut settings, "port", self.port.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "ble_gatt" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "peripheral_id", self.peripheral_id.as_ref());
                insert_opt_string(&mut settings, "service_uuid", self.service_uuid.as_ref());
                insert_opt_string(&mut settings, "write_char_uuid", self.write_char_uuid.as_ref());
                insert_opt_string(
                    &mut settings,
                    "notify_char_uuid",
                    self.notify_char_uuid.as_ref(),
                );
                insert_opt_u64(&mut settings, "scan_timeout_ms", self.scan_timeout_ms);
                insert_opt_u64(
                    &mut settings,
                    "ble_connect_timeout_ms",
                    self.ble_connect_timeout_ms,
                );
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "vrn76_kiss_ble" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "peripheral_id", self.peripheral_id.as_ref());
                insert_opt_string(&mut settings, "frame_mode", self.frame_mode.as_ref());
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(
                    &mut settings,
                    "max_write_len",
                    self.max_write_len.map(|v| v as u64),
                );
                insert_opt_u64(&mut settings, "preamble_ms", self.preamble_ms.map(u64::from));
                insert_opt_u64(&mut settings, "tx_tail_ms", self.tx_tail_ms.map(u64::from));
                insert_opt_u64(&mut settings, "persistence", self.persistence.map(u64::from));
                insert_opt_u64(&mut settings, "slot_time_ms", self.slot_time_ms.map(u64::from));
                if let Some(flow_control) = self.kiss_flow_control {
                    settings.insert("kiss_flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "scan_timeout_ms", self.scan_timeout_ms);
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "lora" => {
                insert_opt_string(&mut settings, "adapter", self.adapter.as_ref());
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(
                    &mut settings,
                    "max_write_len",
                    self.max_write_len.map(|v| v as u64),
                );
                insert_opt_string(&mut settings, "region", self.region.as_ref());
                insert_opt_u64(&mut settings, "frequency_hz", self.frequency_hz);
                insert_opt_u64(&mut settings, "bandwidth_hz", self.bandwidth_hz.map(u64::from));
                insert_opt_u64(
                    &mut settings,
                    "spreading_factor",
                    self.spreading_factor.map(u64::from),
                );
                insert_opt_string(&mut settings, "coding_rate", self.coding_rate.as_ref());
                if let Some(tx_power_dbm) = self.tx_power_dbm {
                    settings
                        .insert("tx_power_dbm".to_string(), JsonValue::Number(tx_power_dbm.into()));
                }
                if let Some(flow_control) =
                    self.flow_control.as_ref().and_then(toml::Value::as_bool)
                {
                    settings.insert("flow_control".to_string(), JsonValue::Bool(flow_control));
                }
                insert_opt_u64(&mut settings, "scan_timeout_ms", self.scan_timeout_ms);
                insert_opt_u64(
                    &mut settings,
                    "ble_connect_timeout_ms",
                    self.ble_connect_timeout_ms,
                );
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_string(&mut settings, "id_callsign", self.id_callsign.as_ref());
                insert_opt_u64(&mut settings, "id_interval", self.id_interval);
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
                insert_opt_f64(&mut settings, "airtime_limit_short", self.airtime_limit_short);
                insert_opt_f64(&mut settings, "airtime_limit_long", self.airtime_limit_long);
                insert_opt_u64(&mut settings, "sync_word", self.sync_word.map(u64::from));
                insert_opt_u64(
                    &mut settings,
                    "preamble_symbols",
                    self.preamble_symbols.map(u64::from),
                );
                insert_opt_u64(
                    &mut settings,
                    "max_payload_bytes",
                    self.max_payload_bytes.map(u64::from),
                );
                insert_opt_string(&mut settings, "state_path", self.state_path.as_ref());
            }
            _ => {}
        }
        (!settings.is_empty()).then_some(JsonValue::Object(settings))
    }

    fn validate(&self, index: usize, original_kind: &str) -> Result<(), String> {
        let kind = self.kind.trim();
        if kind.is_empty() {
            return Err(format!("interfaces[{index}].type is required"));
        }
        self.interface_mode().map_err(|err| format!("interfaces[{index}].{err}"))?;
        self.validate_announce_pacing(index)?;
        match kind {
            "udp" => self.validate_udp(index),
            "auto" => self.validate_auto(index),
            "serial" => self.validate_serial(index),
            "kiss" => self.validate_kiss(index),
            "kiss_tcp_client" => self.validate_kiss_tcp_client(index),
            "ble_gatt" => self.validate_ble(index),
            "vrn76_kiss_ble" => self.validate_vrn76_kiss_ble(index),
            "lora" => self.validate_lora(index, original_kind),
            _ if is_known_unsupported_python_interface(original_kind) => Err(format!(
                "interfaces[{index}].type {original_kind} is a known unsupported Reticulum interface family"
            )),
            _ => Ok(()),
        }
    }

    pub fn interface_mode(&self) -> Result<rns_transport::iface::InterfaceMode, String> {
        let Some((field, value)) = self.interface_mode_raw() else {
            return Ok(rns_transport::iface::InterfaceMode::Full);
        };
        rns_transport::iface::InterfaceMode::parse(value).ok_or_else(|| {
            format!(
                "{field} must be one of full, access_point, accesspoint, ap, pointtopoint, ptp, roaming, boundary, gateway, gw"
            )
        })
    }

    fn interface_mode_raw(&self) -> Option<(&'static str, &str)> {
        self.interface_mode
            .as_deref()
            .map(|value| ("interface_mode", value))
            .or_else(|| self.mode.as_deref().map(|value| ("mode", value)))
    }

    pub fn flow_control_name(&self) -> Option<&str> {
        self.flow_control.as_ref().and_then(toml::Value::as_str)
    }

    pub fn auto_discovery_multicast_address(&self) -> Option<String> {
        if self.kind != "auto" {
            return None;
        }
        let scope = rns_transport::iface::auto::AutoDiscoveryScope::parse(
            self.discovery_scope.as_deref()?,
        )?;
        let address_type = rns_transport::iface::auto::MulticastAddressType::parse(
            self.multicast_address_type.as_deref()?,
        )?;
        Some(rns_transport::iface::auto::multicast_discovery_address(
            self.group_id.as_deref()?.as_bytes(),
            scope,
            address_type,
        ))
    }

    fn validate_announce_pacing(&self, index: usize) -> Result<(), String> {
        if self.bitrate == Some(0) {
            return Err(format!("interfaces[{index}].bitrate must be > 0"));
        }
        if let Some(announce_cap) = self.announce_cap {
            if !(1..=100).contains(&announce_cap) {
                return Err(format!("interfaces[{index}].announce_cap must be between 1 and 100"));
            }
        }
        Ok(())
    }

    fn normalize_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        self.normalize_port_alias(index)?;
        if self.kind == "tcp_client" {
            self.normalize_tcp_client_aliases(index)?;
        }
        if self.kind == "tcp_server" {
            self.normalize_tcp_server_aliases(index)?;
        }
        if self.kind == "udp" {
            self.normalize_udp_aliases(index)?;
        }
        if self.kind == "auto" {
            self.normalize_auto_aliases(index)?;
        }
        if self.kind == "serial" {
            self.normalize_serial_aliases(index)?;
        }
        if self.kind == "vrn76_kiss_ble" {
            self.normalize_vrn76_kiss_ble_aliases(index)?;
        }
        if self.kind == "kiss" {
            self.normalize_kiss_aliases(index, original_kind)?;
        }
        if self.kind == "lora" {
            self.normalize_lora_aliases(index, original_kind)?;
        }
        Ok(())
    }

    fn normalize_port_alias(&mut self, index: usize) -> Result<(), String> {
        let Some(value) = self.extra.remove("port") else {
            return Ok(());
        };
        match self.kind.as_str() {
            "tcp_client" | "tcp_server" | "udp" | "kiss_tcp_client" => {
                if self.port.is_none() {
                    self.port = Some(port_number_from_value(value, index)?);
                }
            }
            "serial" | "kiss" | "lora" => {
                if self.device.is_none() {
                    self.device =
                        Some(string_from_value(value, "port", index, self.kind.as_str())?);
                }
            }
            _ => {
                self.extra.insert("port".to_string(), value);
            }
        }
        Ok(())
    }

    fn normalize_tcp_client_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.host.is_none() {
            self.host = self.target_host.clone().and_then(non_empty_string);
        }
        if self.port.is_none() {
            self.port = self.target_port;
        }
        if self.mtu.is_none() {
            self.mtu = self
                .take_u64_alias_for_kind("fixed_mtu", index, "tcp_client")?
                .map(|value| {
                    usize::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].fixed_mtu must fit in usize for tcp_client")
                    })
                })
                .transpose()?;
        } else {
            let _ = self.take_u64_alias_for_kind("fixed_mtu", index, "tcp_client")?;
        }
        if self.take_bool_alias_for_kind("kiss_framing", index, "tcp_client")?.unwrap_or(false) {
            self.kind = "kiss_tcp_client".to_string();
        }
        Ok(())
    }

    fn normalize_tcp_server_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.host.is_none() {
            self.host = self.take_string_alias_for_kind("listen_ip", index, "tcp_server")?;
        } else {
            let _ = self.take_string_alias_for_kind("listen_ip", index, "tcp_server")?;
        }
        if self.port.is_none() {
            self.port = self.take_u16_alias_for_kind("listen_port", index, "tcp_server")?;
        } else {
            let _ = self.take_u16_alias_for_kind("listen_port", index, "tcp_server")?;
        }
        Ok(())
    }

    fn normalize_udp_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.host.is_none() {
            self.host = self.take_string_alias_for_kind("listen_ip", index, "udp")?;
        } else {
            let _ = self.take_string_alias_for_kind("listen_ip", index, "udp")?;
        }
        if self.port.is_none() {
            self.port = self.take_u16_alias_for_kind("listen_port", index, "udp")?;
        } else {
            let _ = self.take_u16_alias_for_kind("listen_port", index, "udp")?;
        }
        let used_forward_ip_alias = if self.target_host.is_none() {
            let forward_ip = self.take_string_alias_for_kind("forward_ip", index, "udp")?;
            let used = forward_ip.is_some();
            self.target_host = forward_ip;
            used
        } else {
            let _ = self.take_string_alias_for_kind("forward_ip", index, "udp")?;
            false
        };
        if self.target_port.is_none() {
            self.target_port = self.take_u16_alias_for_kind("forward_port", index, "udp")?;
        } else {
            let _ = self.take_u16_alias_for_kind("forward_port", index, "udp")?;
        }
        if used_forward_ip_alias && self.target_port.is_none() {
            self.target_port = self.port;
        }
        Ok(())
    }

    fn normalize_auto_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.group_id.is_none() {
            self.group_id = Some("reticulum".to_string());
        }
        if self.discovery_scope.is_none() {
            self.discovery_scope = Some("link".to_string());
        }
        if self.discovery_port.is_none() {
            self.discovery_port = Some(29_716);
        }
        if self.data_port.is_none() {
            self.data_port = Some(42_671);
        }
        if self.multicast_address_type.is_none() {
            self.multicast_address_type = Some("temporary".to_string());
        }
        if self.bitrate.is_none() {
            self.bitrate = self.take_u64_alias_for_kind("configured_bitrate", index, "auto")?;
        } else {
            let _ = self.take_u64_alias_for_kind("configured_bitrate", index, "auto")?;
        }
        Ok(())
    }

    fn normalize_serial_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "serial")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].speed must fit in u32 for serial")
                    })
                })
                .transpose()?;
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "serial")?;
        }
        if self.data_bits.is_none() {
            self.data_bits = self.take_u8_alias_for_kind("databits", index, "serial")?;
        } else {
            let _ = self.take_u8_alias_for_kind("databits", index, "serial")?;
        }
        if self.stop_bits.is_none() {
            self.stop_bits = self.take_u8_alias_for_kind("stopbits", index, "serial")?;
        } else {
            let _ = self.take_u8_alias_for_kind("stopbits", index, "serial")?;
        }
        Ok(())
    }

    fn normalize_lora_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        if self.frequency_hz.is_none() {
            self.frequency_hz = self.take_u64_alias_for_kind("frequency", index, "lora")?;
        } else {
            let _ = self.take_u64_alias_for_kind("frequency", index, "lora")?;
        }
        if self.bandwidth_hz.is_none() {
            self.bandwidth_hz = self
                .take_u64_alias_for_kind("bandwidth", index, "lora")?
                .map(|value| {
                    u32::try_from(value).map_err(|_| {
                        format!("interfaces[{index}].bandwidth must fit in u32 for lora")
                    })
                })
                .transpose()?;
        } else {
            let _ = self.take_u64_alias_for_kind("bandwidth", index, "lora")?;
        }
        if self.spreading_factor.is_none() {
            self.spreading_factor =
                self.take_u8_alias_for_kind("spreadingfactor", index, "lora")?;
        } else {
            let _ = self.take_u8_alias_for_kind("spreadingfactor", index, "lora")?;
        }
        if self.coding_rate.is_none() {
            self.coding_rate = self.take_string_or_integer_alias("codingrate", index, "lora")?;
        } else {
            let _ = self.take_string_or_integer_alias("codingrate", index, "lora")?;
        }
        if self.tx_power_dbm.is_none() {
            self.tx_power_dbm = self.take_i8_alias_for_kind("txpower", index, "lora")?;
        } else {
            let _ = self.take_i8_alias_for_kind("txpower", index, "lora")?;
        }
        if self.connect_timeout_ms.is_none() {
            self.connect_timeout_ms =
                self.take_u64_alias_for_kind("command_timeout_ms", index, "lora")?;
        } else {
            let _ = self.take_u64_alias_for_kind("command_timeout_ms", index, "lora")?;
        }
        if self.baud_rate.is_none()
            && original_kind == "RNodeInterface"
            && self
                .device
                .as_deref()
                .is_some_and(|device| !is_tcp_lora_port(device) && !is_ble_lora_port(device))
        {
            self.baud_rate = Some(115_200);
        }
        Ok(())
    }

    fn normalize_kiss_aliases(&mut self, index: usize, original_kind: &str) -> Result<(), String> {
        if self.baud_rate.is_none() {
            self.baud_rate = self
                .take_u64_alias_for_kind("speed", index, "kiss")?
                .map(|value| {
                    u32::try_from(value)
                        .map_err(|_| format!("interfaces[{index}].speed must fit in u32 for kiss"))
                })
                .transpose()?;
            if self.baud_rate.is_none() && original_kind == "KISSInterface" {
                self.baud_rate = Some(9_600);
            }
        } else {
            let _ = self.take_u64_alias_for_kind("speed", index, "kiss")?;
        }
        if self.data_bits.is_none() {
            self.data_bits = self.take_u8_alias_for_kind("databits", index, "kiss")?;
        } else {
            let _ = self.take_u8_alias_for_kind("databits", index, "kiss")?;
        }
        if self.stop_bits.is_none() {
            self.stop_bits = self.take_u8_alias_for_kind("stopbits", index, "kiss")?;
        } else {
            let _ = self.take_u8_alias_for_kind("stopbits", index, "kiss")?;
        }
        if self.preamble_ms.is_none() {
            self.preamble_ms = self.take_u16_alias_for_kind("preamble", index, "kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("preamble", index, "kiss")?;
        }
        if self.tx_tail_ms.is_none() {
            self.tx_tail_ms = self.take_u16_alias_for_kind("txtail", index, "kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("txtail", index, "kiss")?;
        }
        if self.slot_time_ms.is_none() {
            self.slot_time_ms = self.take_u16_alias_for_kind("slottime", index, "kiss")?;
        } else {
            let _ = self.take_u16_alias_for_kind("slottime", index, "kiss")?;
        }
        if self.kiss_flow_control.is_none() {
            if let Some(flow_control) = self.flow_control.as_ref().and_then(toml::Value::as_bool) {
                self.kiss_flow_control = Some(flow_control);
            }
        }
        Ok(())
    }

    fn normalize_vrn76_kiss_ble_aliases(&mut self, index: usize) -> Result<(), String> {
        if self.peripheral_id.is_none() {
            let device_address = self.take_string_alias("device_address", index)?;
            let device_name_filter = self.take_string_alias("device_name_filter", index)?;
            self.peripheral_id = device_address
                .and_then(non_empty_string)
                .or_else(|| device_name_filter.and_then(non_empty_string));
        } else {
            let _ = self.take_string_alias("device_address", index)?;
            let _ = self.take_string_alias("device_name_filter", index)?;
        }
        if self.scan_timeout_ms.is_none() {
            self.scan_timeout_ms = self.take_u64_alias("ble_scan_timeout_ms", index)?;
        } else {
            let _ = self.take_u64_alias("ble_scan_timeout_ms", index)?;
        }
        if self.connect_timeout_ms.is_none() {
            self.connect_timeout_ms = self.take_u64_alias("command_timeout_ms", index)?;
        } else {
            let _ = self.take_u64_alias("command_timeout_ms", index)?;
        }
        if self.preamble_ms.is_none() {
            self.preamble_ms = self.take_u16_alias("preamble", index)?;
        } else {
            let _ = self.take_u16_alias("preamble", index)?;
        }
        if self.tx_tail_ms.is_none() {
            self.tx_tail_ms = self.take_u16_alias("txtail", index)?;
        } else {
            let _ = self.take_u16_alias("txtail", index)?;
        }
        if self.slot_time_ms.is_none() {
            self.slot_time_ms = self.take_u16_alias("slottime", index)?;
        } else {
            let _ = self.take_u16_alias("slottime", index)?;
        }
        if self.kiss_flow_control.is_none() {
            if let Some(flow_control) = self.flow_control.as_ref().and_then(toml::Value::as_bool) {
                self.kiss_flow_control = Some(flow_control);
            }
        }
        Ok(())
    }

    fn take_string_alias(&mut self, key: &str, index: usize) -> Result<Option<String>, String> {
        self.take_string_alias_for_kind(key, index, "vrn76_kiss_ble")
    }

    fn take_string_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<String>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value
            .as_str()
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| format!("interfaces[{index}].{key} must be a string for {kind}"))
    }

    fn take_u64_alias(&mut self, key: &str, index: usize) -> Result<Option<u64>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value.as_integer().and_then(|value| u64::try_from(value).ok()).map(Some).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a non-negative integer for vrn76_kiss_ble")
        })
    }

    fn take_u64_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<u64>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value.as_integer().and_then(|value| u64::try_from(value).ok()).map(Some).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a non-negative integer for {kind}")
        })
    }

    fn take_u8_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<u8>, String> {
        self.take_u64_alias_for_kind(key, index, kind)?
            .map(|value| {
                u8::try_from(value)
                    .map_err(|_| format!("interfaces[{index}].{key} must fit in u8 for {kind}"))
            })
            .transpose()
    }

    fn take_i8_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<i8>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value
            .as_integer()
            .and_then(|value| i8::try_from(value).ok())
            .map(Some)
            .ok_or_else(|| format!("interfaces[{index}].{key} must fit in i8 for {kind}"))
    }

    fn take_bool_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<bool>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("interfaces[{index}].{key} must be a boolean for {kind}"))
    }

    fn take_string_or_integer_alias(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<String>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        if let Some(value) = value.as_str() {
            return Ok(Some(value.to_string()));
        }
        value.as_integer().map(|value| Some(value.to_string())).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a string or integer for {kind}")
        })
    }

    fn take_u16_alias(&mut self, key: &str, index: usize) -> Result<Option<u16>, String> {
        let Some(value) = self.extra.remove(key) else {
            return Ok(None);
        };
        value.as_integer().and_then(|value| u16::try_from(value).ok()).map(Some).ok_or_else(|| {
            format!("interfaces[{index}].{key} must be a 16-bit integer for vrn76_kiss_ble")
        })
    }

    fn take_u16_alias_for_kind(
        &mut self,
        key: &str,
        index: usize,
        kind: &str,
    ) -> Result<Option<u16>, String> {
        self.take_u64_alias_for_kind(key, index, kind)?
            .map(|value| {
                u16::try_from(value)
                    .map_err(|_| format!("interfaces[{index}].{key} must fit in u16 for {kind}"))
            })
            .transpose()
    }

    fn validate_udp(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "udp")?;
        if !self.enabled() {
            return Ok(());
        }
        let has_bind_host =
            self.host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        if !has_bind_host {
            return Err(format!("interfaces[{index}].host is required for udp"));
        }
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for udp"));
        }
        let has_target_host =
            self.target_host.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        if has_target_host ^ self.target_port.is_some() {
            return Err(format!(
                "interfaces[{index}].target_host and target_port must be provided together for udp"
            ));
        }
        Ok(())
    }

    fn validate_auto(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "auto")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.group_id.as_deref(),
            &format!("interfaces[{index}].group_id is required for auto"),
        )?;
        if rns_transport::iface::auto::AutoDiscoveryScope::parse(
            self.discovery_scope.as_deref().unwrap_or_default(),
        )
        .is_none()
        {
            return Err(format!(
                "interfaces[{index}].discovery_scope must be one of link, admin, site, organisation, organization, global for auto"
            ));
        }
        if rns_transport::iface::auto::MulticastAddressType::parse(
            self.multicast_address_type.as_deref().unwrap_or_default(),
        )
        .is_none()
        {
            return Err(format!(
                "interfaces[{index}].multicast_address_type must be temporary or permanent for auto"
            ));
        }
        if self.discovery_port == Some(0) {
            return Err(format!("interfaces[{index}].discovery_port must be > 0 for auto"));
        }
        if self.data_port == Some(0) {
            return Err(format!("interfaces[{index}].data_port must be > 0 for auto"));
        }
        Ok(())
    }

    fn validate_serial(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "serial")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device is required for serial"),
        )?;
        if self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for serial"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for serial"));
        }
        if let Some(data_bits) = self.data_bits {
            if !(5..=8).contains(&data_bits) {
                return Err(format!(
                    "interfaces[{index}].data_bits must be one of 5, 6, 7, 8 for serial"
                ));
            }
        }
        if let Some(stop_bits) = self.stop_bits {
            if stop_bits != 1 && stop_bits != 2 {
                return Err(format!(
                    "interfaces[{index}].stop_bits must be one of 1, 2 for serial"
                ));
            }
        }
        if let Some(parity) = self.parity.as_deref() {
            if !matches_normalized(parity, &["n", "none", "e", "even", "o", "odd"]) {
                return Err(format!(
                    "interfaces[{index}].parity must be one of n, none, e, even, o, odd for serial"
                ));
            }
        }
        if let Some(flow_control) = self.flow_control.as_ref() {
            let Some(flow_control) = flow_control.as_str() else {
                return Err(format!(
                    "interfaces[{index}].flow_control must be one of none, software, hardware for serial"
                ));
            };
            if !matches_normalized(flow_control, &["none", "software", "hardware"]) {
                return Err(format!(
                    "interfaces[{index}].flow_control must be one of none, software, hardware for serial"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(256..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 256 and 65535 for serial"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for serial"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for serial"
                ));
            }
        }
        Ok(())
    }

    fn validate_kiss(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "kiss")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.device.as_deref(),
            &format!("interfaces[{index}].device is required for kiss"),
        )?;
        if self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for kiss"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for kiss"));
        }
        self.validate_id_beacon(index, "kiss")?;
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for kiss"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for kiss"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for kiss"
                ));
            }
        }
        Ok(())
    }

    fn validate_kiss_tcp_client(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "kiss_tcp_client")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.host.as_deref(),
            &format!("interfaces[{index}].host is required for kiss_tcp_client"),
        )?;
        if self.port.is_none() {
            return Err(format!("interfaces[{index}].port is required for kiss_tcp_client"));
        }
        if self.port == Some(0) {
            return Err(format!("interfaces[{index}].port must be > 0 for kiss_tcp_client"));
        }
        self.validate_id_beacon(index, "kiss_tcp_client")?;
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for kiss_tcp_client"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for kiss_tcp_client"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for kiss_tcp_client"
                ));
            }
        }
        Ok(())
    }

    fn validate_ble(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "ble_gatt")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.peripheral_id.as_deref(),
            &format!("interfaces[{index}].peripheral_id is required for ble_gatt"),
        )?;
        require_non_empty(
            self.service_uuid.as_deref(),
            &format!("interfaces[{index}].service_uuid is required for ble_gatt"),
        )?;
        require_non_empty(
            self.write_char_uuid.as_deref(),
            &format!("interfaces[{index}].write_char_uuid is required for ble_gatt"),
        )?;
        require_non_empty(
            self.notify_char_uuid.as_deref(),
            &format!("interfaces[{index}].notify_char_uuid is required for ble_gatt"),
        )?;
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for ble_gatt"),
            )?;
        }
        let service_uuid = self.service_uuid.as_deref().unwrap_or_default();
        if !is_uuid_like(service_uuid) {
            return Err(format!(
                "interfaces[{index}].service_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"
            ));
        }
        let write_char_uuid = self.write_char_uuid.as_deref().unwrap_or_default();
        if !is_uuid_like(write_char_uuid) {
            return Err(format!(
                "interfaces[{index}].write_char_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"
            ));
        }
        let notify_char_uuid = self.notify_char_uuid.as_deref().unwrap_or_default();
        if !is_uuid_like(notify_char_uuid) {
            return Err(format!(
                "interfaces[{index}].notify_char_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"
            ));
        }
        if let Some(scan_timeout_ms) = self.scan_timeout_ms {
            if scan_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].scan_timeout_ms must be > 0 for ble_gatt"
                ));
            }
        }
        if let Some(connect_timeout_ms) = self.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].connect_timeout_ms must be > 0 for ble_gatt"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(23..=517).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 23 and 517 for ble_gatt"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for ble_gatt"
                ));
            }
        }
        Ok(())
    }

    fn validate_vrn76_kiss_ble(&self, index: usize) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "vrn76_kiss_ble")?;
        if let Some(flow_control) = self.flow_control.as_ref() {
            if !flow_control.is_bool() {
                return Err(format!(
                    "interfaces[{index}].flow_control must be a boolean for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(frame_mode) = self.frame_mode.as_deref() {
            if !matches_vrn76_frame_mode(frame_mode) {
                return Err(format!(
                    "interfaces[{index}].frame_mode must be one of benshi_tnc_data, benshi, raw_kiss, raw for vrn76_kiss_ble"
                ));
            }
        }
        self.validate_id_beacon(index, "vrn76_kiss_ble")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.peripheral_id.as_deref(),
            &format!("interfaces[{index}].peripheral_id is required for vrn76_kiss_ble"),
        )?;
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for vrn76_kiss_ble"),
            )?;
        }
        if let Some(scan_timeout_ms) = self.scan_timeout_ms {
            if scan_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].scan_timeout_ms must be > 0 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(connect_timeout_ms) = self.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].connect_timeout_ms must be > 0 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if !(64..=65535).contains(&mtu) {
                return Err(format!(
                    "interfaces[{index}].mtu must be between 64 and 65535 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(max_write_len) = self.max_write_len {
            if !(6..=65535).contains(&max_write_len) {
                return Err(format!(
                    "interfaces[{index}].max_write_len must be between 6 and 65535 for vrn76_kiss_ble"
                ));
            }
        }
        if let Some(reconnect_backoff_ms) = self.reconnect_backoff_ms {
            if reconnect_backoff_ms < 50 {
                return Err(format!(
                    "interfaces[{index}].reconnect_backoff_ms must be >= 50 for vrn76_kiss_ble"
                ));
            }
        }
        if let (Some(reconnect_backoff_ms), Some(max_reconnect_backoff_ms)) =
            (self.reconnect_backoff_ms, self.max_reconnect_backoff_ms)
        {
            if max_reconnect_backoff_ms < reconnect_backoff_ms {
                return Err(format!(
                    "interfaces[{index}].max_reconnect_backoff_ms must be >= reconnect_backoff_ms for vrn76_kiss_ble"
                ));
            }
        }
        Ok(())
    }

    fn validate_lora(&self, index: usize, original_kind: &str) -> Result<(), String> {
        self.reject_unknown_new_kind_keys(index, "lora")?;
        if !self.enabled() {
            return Ok(());
        }
        require_non_empty(
            self.region.as_deref(),
            &format!("interfaces[{index}].region is required for lora"),
        )?;
        let region = self.region.as_deref().unwrap_or_default();
        if !is_supported_lora_region(region) {
            return Err(format!(
                "interfaces[{index}].region must be one of EU868, US915, AU915, AS923, IN865, KR920, RU864 for lora"
            ));
        }
        if self.state_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_none() {
            return Err(format!("interfaces[{index}].state_path is required for lora"));
        }
        let has_device =
            self.device.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        let has_tcp_device = self.device.as_deref().is_some_and(is_tcp_lora_port);
        let has_ble_device = self.device.as_deref().is_some_and(is_ble_lora_port);
        if has_device && !has_tcp_device && !has_ble_device && self.baud_rate.is_none() {
            return Err(format!("interfaces[{index}].baud_rate is required for active lora"));
        }
        if !has_device && self.baud_rate.is_some() {
            return Err(format!("interfaces[{index}].device is required for active lora"));
        }
        if self.baud_rate == Some(0) {
            return Err(format!("interfaces[{index}].baud_rate must be > 0 for lora"));
        }
        if let Some(adapter) = self.adapter.as_deref() {
            require_non_empty(
                Some(adapter),
                &format!("interfaces[{index}].adapter cannot be empty for lora"),
            )?;
        }
        if original_kind == "RNodeInterface" {
            self.validate_rnode_required_radio_parameters(index)?;
        }
        if let Some(scan_timeout_ms) = self.scan_timeout_ms {
            if scan_timeout_ms == 0 {
                return Err(format!("interfaces[{index}].scan_timeout_ms must be > 0 for lora"));
            }
        }
        if let Some(connect_timeout_ms) = self.connect_timeout_ms {
            if connect_timeout_ms == 0 {
                return Err(format!("interfaces[{index}].connect_timeout_ms must be > 0 for lora"));
            }
        }
        if let Some(ble_connect_timeout_ms) = self.ble_connect_timeout_ms {
            if ble_connect_timeout_ms == 0 {
                return Err(format!(
                    "interfaces[{index}].ble_connect_timeout_ms must be > 0 for lora"
                ));
            }
        }
        if let Some(mtu) = self.mtu {
            if mtu == 0 {
                return Err(format!("interfaces[{index}].mtu must be > 0 for lora"));
            }
        }
        if let Some(max_write_len) = self.max_write_len {
            if max_write_len == 0 {
                return Err(format!("interfaces[{index}].max_write_len must be > 0 for lora"));
            }
        }
        self.validate_id_beacon(index, "lora")?;
        if let Some(flow_control) = self.flow_control.as_ref() {
            if !flow_control.is_bool() {
                return Err(format!("interfaces[{index}].flow_control must be a boolean for lora"));
            }
        }
        if let Some(frequency_hz) = self.frequency_hz {
            if !(137_000_000..=3_000_000_000).contains(&frequency_hz) {
                return Err(format!(
                    "interfaces[{index}].frequency_hz must be between 137000000 and 3000000000 for lora"
                ));
            }
        }
        if let Some(spreading_factor) = self.spreading_factor {
            if !(5..=12).contains(&spreading_factor) {
                return Err(format!(
                    "interfaces[{index}].spreading_factor must be between 5 and 12 for lora"
                ));
            }
        }
        if let Some(coding_rate) = self.coding_rate.as_deref() {
            if !matches_normalized(coding_rate, &["4/5", "4/6", "4/7", "4/8", "5", "6", "7", "8"]) {
                return Err(format!(
                    "interfaces[{index}].coding_rate must be one of 4/5, 4/6, 4/7, 4/8, 5, 6, 7, 8 for lora"
                ));
            }
        }
        if let Some(bandwidth_hz) = self.bandwidth_hz {
            if !(7_800..=1_625_000).contains(&bandwidth_hz) {
                return Err(format!(
                    "interfaces[{index}].bandwidth_hz must be between 7800 and 1625000 for lora"
                ));
            }
        }
        if let Some(tx_power_dbm) = self.tx_power_dbm {
            if !(0..=37).contains(&tx_power_dbm) {
                return Err(format!(
                    "interfaces[{index}].tx_power_dbm must be between 0 and 37 for lora"
                ));
            }
        }
        if let Some(max_payload_bytes) = self.max_payload_bytes {
            if !(1..=255).contains(&max_payload_bytes) {
                return Err(format!(
                    "interfaces[{index}].max_payload_bytes must be between 1 and 255 for lora"
                ));
            }
        }
        if let Some(airtime_limit_short) = self.airtime_limit_short {
            if !(0.0..=100.0).contains(&airtime_limit_short) {
                return Err(format!(
                    "interfaces[{index}].airtime_limit_short must be between 0 and 100 for lora"
                ));
            }
        }
        if let Some(airtime_limit_long) = self.airtime_limit_long {
            if !(0.0..=100.0).contains(&airtime_limit_long) {
                return Err(format!(
                    "interfaces[{index}].airtime_limit_long must be between 0 and 100 for lora"
                ));
            }
        }
        Ok(())
    }

    fn validate_rnode_required_radio_parameters(&self, index: usize) -> Result<(), String> {
        if self.frequency_hz.is_none() {
            return Err(format!("interfaces[{index}].frequency is required for RNodeInterface"));
        }
        if self.bandwidth_hz.is_none() {
            return Err(format!("interfaces[{index}].bandwidth is required for RNodeInterface"));
        }
        if self.spreading_factor.is_none() {
            return Err(format!(
                "interfaces[{index}].spreadingfactor is required for RNodeInterface"
            ));
        }
        if self.coding_rate.is_none() {
            return Err(format!("interfaces[{index}].codingrate is required for RNodeInterface"));
        }
        Ok(())
    }

    fn validate_id_beacon(&self, index: usize, kind: &str) -> Result<(), String> {
        if let Some(callsign) = self.id_callsign.as_deref() {
            let callsign = callsign.trim();
            if callsign.is_empty() {
                return Err(format!("interfaces[{index}].id_callsign cannot be empty for {kind}"));
            }
            if callsign.len() > 32 {
                return Err(format!(
                    "interfaces[{index}].id_callsign must be 32 bytes or fewer for {kind}"
                ));
            }
        }
        if self.id_interval == Some(0) {
            return Err(format!("interfaces[{index}].id_interval must be > 0 for {kind}"));
        }
        Ok(())
    }

    fn reject_unknown_new_kind_keys(&self, index: usize, kind: &str) -> Result<(), String> {
        self.reject_unknown_new_kind_keys_except(index, kind, &[])
    }

    fn reject_unknown_new_kind_keys_except(
        &self,
        index: usize,
        kind: &str,
        allowed: &[&str],
    ) -> Result<(), String> {
        if self.extra.is_empty() {
            return Ok(());
        }
        let mut unknown = self
            .extra
            .keys()
            .filter(|key| !allowed.iter().any(|allowed| allowed == &key.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if unknown.is_empty() {
            return Ok(());
        }
        unknown.sort();
        Err(format!(
            "interfaces[{index}] ({kind}) contains unknown settings key(s): {}",
            unknown.join(", ")
        ))
    }
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn string_from_value(
    value: toml::Value,
    key: &str,
    index: usize,
    kind: &str,
) -> Result<String, String> {
    value
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| format!("interfaces[{index}].{key} must be a string for {kind}"))
}

fn port_number_from_value(value: toml::Value, index: usize) -> Result<u16, String> {
    value
        .as_integer()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| format!("interfaces[{index}].port must be a 16-bit integer"))
}

fn deserialize_optional_string_list<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<toml::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        toml::Value::String(value) => Ok(Some(split_string_list(&value))),
        toml::Value::Array(items) => items
            .into_iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    D::Error::custom("interface device list entries must be strings")
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        _ => Err(D::Error::custom("interface device list must be a string or string array")),
    }
}

fn split_string_list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).collect()
}

fn normalize_interface_kind(value: &str) -> String {
    match value {
        "AutoInterface" => "auto".to_string(),
        "TCPClientInterface" => "tcp_client".to_string(),
        "TCPServerInterface" => "tcp_server".to_string(),
        "UDPInterface" => "udp".to_string(),
        "SerialInterface" => "serial".to_string(),
        "KISSInterface" => "kiss".to_string(),
        "RNodeInterface" => "lora".to_string(),
        "Vrn76KissBluetoothInterface" | "Vrn76KissBleInterface" => "vrn76_kiss_ble".to_string(),
        value => value.to_string(),
    }
}

fn is_known_unsupported_python_interface(value: &str) -> bool {
    matches!(
        value,
        "PipeInterface"
            | "LocalInterface"
            | "I2PInterface"
            | "WeaveInterface"
            | "BackboneInterface"
    )
}

fn is_tcp_lora_port(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("tcp://")
}

fn is_ble_lora_port(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("ble://")
}

fn deserialize_interfaces<'de, D>(deserializer: D) -> Result<Vec<InterfaceConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = toml::Value::deserialize(deserializer)?;
    match value {
        toml::Value::Array(items) => items.into_iter().map(interface_from_value).collect(),
        toml::Value::Table(table) => table
            .into_iter()
            .map(|(key, value)| {
                let toml::Value::Table(mut interface) = value else {
                    return Err(D::Error::custom(format!(
                        "interfaces.{key} must be an interface settings table"
                    )));
                };
                if !interface.contains_key("name") {
                    interface.insert("name".to_string(), toml::Value::String(key.clone()));
                }
                if !interface.contains_key("type") {
                    interface.insert("type".to_string(), toml::Value::String(key));
                }
                interface_from_value(toml::Value::Table(interface))
            })
            .collect(),
        other => Err(D::Error::custom(format!(
            "interfaces must be an array or table, got {}",
            other.type_str()
        ))),
    }
}

fn interface_from_value<E>(value: toml::Value) -> Result<InterfaceConfig, E>
where
    E: DeError,
{
    value.try_into().map_err(E::custom)
}

fn require_non_empty(value: Option<&str>, error: &str) -> Result<(), String> {
    if value.is_some_and(|item| !item.trim().is_empty()) {
        Ok(())
    } else {
        Err(error.to_string())
    }
}

fn insert_opt_string(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<&String>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::String(value.clone()));
    }
}

fn insert_opt_string_array(
    target: &mut JsonMap<String, JsonValue>,
    key: &str,
    value: Option<&Vec<String>>,
) {
    if let Some(value) = value {
        target.insert(
            key.to_string(),
            JsonValue::Array(value.iter().cloned().map(JsonValue::String).collect()),
        );
    }
}

fn insert_opt_u64(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::Number(value.into()));
    }
}

fn insert_opt_bool(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::Bool(value));
    }
}

fn insert_opt_f64(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<f64>) {
    if let Some(value) = value.and_then(serde_json::Number::from_f64) {
        target.insert(key.to_string(), JsonValue::Number(value));
    }
}

fn matches_normalized(value: &str, candidates: &[&str]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    candidates.iter().any(|candidate| normalized == *candidate)
}

fn matches_vrn76_frame_mode(value: &str) -> bool {
    matches_normalized(value, &["benshi_tnc_data", "benshi", "raw_kiss", "raw"])
}

fn is_uuid_like(value: &str) -> bool {
    let normalized = value.trim();
    if normalized.is_empty() {
        return false;
    }

    if normalized.len() == 4 || normalized.len() == 8 {
        return normalized.chars().all(|ch| ch.is_ascii_hexdigit());
    }

    if normalized.len() == 36 {
        let bytes = normalized.as_bytes();
        let hyphen_positions = [8_usize, 13, 18, 23];
        for idx in hyphen_positions {
            if bytes[idx] != b'-' {
                return false;
            }
        }
        return normalized
            .chars()
            .enumerate()
            .all(|(idx, ch)| hyphen_positions.contains(&idx) || ch.is_ascii_hexdigit());
    }

    false
}

fn is_supported_lora_region(region: &str) -> bool {
    matches!(
        region.trim().to_ascii_uppercase().as_str(),
        "EU868" | "US915" | "AU915" | "AS923" | "IN865" | "KR920" | "RU864"
    )
}
