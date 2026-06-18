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
    pub propagation_node: Option<PropagationNodeConfig>,
    pub interfaces: Vec<InterfaceConfig>,
}

#[derive(Debug, Deserialize)]
struct DaemonConfigRaw {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    announce_capabilities: Vec<String>,
    #[serde(default)]
    propagation_node: Option<PropagationNodeConfig>,
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
            propagation_node: raw.propagation_node,
            interfaces,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PropagationNodeConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub node_announce_at_start: Option<bool>,
    #[serde(default)]
    pub node_announce_interval_secs: Option<u64>,
    #[serde(default)]
    pub peer_announce_at_start: Option<bool>,
    #[serde(default)]
    pub peer_announce_interval_secs: Option<u64>,
    #[serde(default)]
    pub transfer_limit_kb: Option<u32>,
    #[serde(default)]
    pub sync_limit_kb: Option<u32>,
    #[serde(default)]
    pub stamp_cost: Option<u32>,
    #[serde(default)]
    pub stamp_cost_flexibility: Option<u32>,
    #[serde(default)]
    pub peering_cost: Option<u32>,
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
    pub detection_fallback_timeout_ms: Option<u64>,
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
