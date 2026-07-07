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
    #[serde(default)]
    reticulum: Option<ReticulumConfigRaw>,
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
        if should_synthesize_global_shared_instance(raw.reticulum.as_ref(), &interfaces) {
            if let Some(reticulum) = raw.reticulum.as_ref() {
                interfaces.push(reticulum.global_shared_instance_interface());
            }
        }
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
#[allow(dead_code)]
struct ReticulumConfigRaw {
    #[serde(default)]
    share_instance: Option<bool>,
    #[serde(default)]
    shared_instance_type: Option<String>,
    #[serde(default)]
    shared_instance_port: Option<u16>,
    #[serde(default)]
    instance_name: Option<String>,
    #[serde(default)]
    force_shared_instance_bitrate: Option<u64>,
    #[serde(default)]
    instance_control_port: Option<u16>,
    #[serde(default)]
    rpc_key: Option<String>,
}

impl ReticulumConfigRaw {
    fn global_shared_instance_interface(&self) -> InterfaceConfig {
        let shared_instance_type = self
            .shared_instance_type
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .filter(|value| matches!(value.as_str(), "tcp" | "unix"))
            .unwrap_or_else(default_global_shared_instance_type);
        let instance_name = self
            .instance_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let is_tcp = shared_instance_type == "tcp";
        InterfaceConfig {
            kind: "local".to_string(),
            enabled: Some(true),
            name: Some("shared-instance".to_string()),
            synthetic_shared_instance: true,
            shared_instance_type: Some(shared_instance_type),
            host: is_tcp.then(|| "127.0.0.1".to_string()),
            port: is_tcp.then_some(self.shared_instance_port.unwrap_or(37_428)),
            instance_name,
            force_shared_instance_bitrate: self.force_shared_instance_bitrate,
            ..InterfaceConfig::default()
        }
    }
}

fn should_synthesize_global_shared_instance(
    reticulum: Option<&ReticulumConfigRaw>,
    interfaces: &[InterfaceConfig],
) -> bool {
    let Some(reticulum) = reticulum else {
        return false;
    };
    if reticulum.share_instance == Some(false) {
        return false;
    }
    !interfaces.iter().any(|iface| {
        matches!(
            normalize_interface_kind(iface.kind.trim()).as_str(),
            "local" | "local_client"
        )
    })
}

#[cfg(any(target_family = "unix", target_os = "android"))]
fn default_global_shared_instance_type() -> String {
    "unix".to_string()
}

#[cfg(not(any(target_family = "unix", target_os = "android")))]
fn default_global_shared_instance_type() -> String {
    "tcp".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PropagationNodeConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub control_allowed: Vec<String>,
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
    pub interface_enabled: Option<bool>,
    #[serde(skip)]
    pub rnode_profile: bool,
    #[serde(skip)]
    pub synthetic_shared_instance: bool,
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
    pub force_shared_instance_bitrate: Option<u64>,
    #[serde(default)]
    pub announce_cap: Option<u64>,
    #[serde(default)]
    pub announce_rate_target: Option<u64>,
    #[serde(default)]
    pub announce_rate_grace: Option<u64>,
    #[serde(default)]
    pub announce_rate_penalty: Option<u64>,
    #[serde(default)]
    pub bootstrap_only: Option<bool>,
    #[serde(default)]
    pub ignore_config_warnings: Option<bool>,
    #[serde(default)]
    pub ifac_size: Option<u64>,
    #[serde(default)]
    pub networkname: Option<String>,
    #[serde(default)]
    pub network_name: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    #[serde(default)]
    pub pass_phrase: Option<String>,
    #[serde(default)]
    pub ingress_control: Option<bool>,
    #[serde(default)]
    pub egress_control: Option<bool>,
    #[serde(default)]
    pub ic_max_held_announces: Option<u64>,
    #[serde(default)]
    pub ic_burst_hold: Option<f64>,
    #[serde(default)]
    pub ic_burst_freq_new: Option<f64>,
    #[serde(default)]
    pub ic_burst_freq: Option<f64>,
    #[serde(default)]
    pub ic_pr_burst_freq_new: Option<f64>,
    #[serde(default)]
    pub ic_pr_burst_freq: Option<f64>,
    #[serde(default)]
    pub ec_pr_freq: Option<f64>,
    #[serde(default)]
    pub ic_new_time: Option<f64>,
    #[serde(default)]
    pub ic_burst_penalty: Option<f64>,
    #[serde(default)]
    pub ic_held_release_interval: Option<f64>,
    #[serde(default)]
    pub discoverable: Option<bool>,
    #[serde(default)]
    pub announce_interval: Option<u64>,
    #[serde(default)]
    pub discovery_stamp_value: Option<u64>,
    #[serde(default)]
    pub discovery_name: Option<String>,
    #[serde(default)]
    pub discovery_encrypt: Option<bool>,
    #[serde(default)]
    pub reachable_on: Option<String>,
    #[serde(default)]
    pub publish_ifac: Option<bool>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub discovery_frequency: Option<u64>,
    #[serde(default)]
    pub discovery_bandwidth: Option<u64>,
    #[serde(default)]
    pub discovery_modulation: Option<u64>,
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
    pub prefer_ipv6: Option<bool>,
    #[serde(default)]
    pub i2p_tunneled: Option<bool>,
    #[serde(default)]
    pub connect_timeout: Option<u64>,
    #[serde(default)]
    pub max_reconnect_tries: Option<u64>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub respawn_delay: Option<f64>,
    #[serde(default)]
    pub shared_instance_type: Option<String>,
    #[serde(default)]
    pub instance_name: Option<String>,
    #[serde(default)]
    pub socket_path: Option<String>,
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
    #[serde(default, deserialize_with = "deserialize_optional_string_list")]
    pub peers: Option<Vec<String>>,
    #[serde(default)]
    pub connectable: Option<bool>,
    #[serde(default)]
    pub sam_host: Option<String>,
    #[serde(default)]
    pub sam_port: Option<u16>,
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
    pub callsign: Option<String>,
    #[serde(default)]
    pub ssid: Option<u8>,
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
    pub allow_bluetooth: Option<bool>,
    #[serde(default)]
    pub target_device_name: Option<String>,
    #[serde(default)]
    pub target_device_address: Option<String>,
    #[serde(default)]
    pub ble_name: Option<String>,
    #[serde(default)]
    pub ble_addr: Option<String>,
    #[serde(default)]
    pub tcp_host: Option<String>,
    #[serde(default)]
    pub force_ble: Option<bool>,
    #[serde(default)]
    pub force_tcp: Option<bool>,
    #[serde(default)]
    pub service_uuid: Option<String>,
    #[serde(default)]
    pub write_char_uuid: Option<String>,
    #[serde(default)]
    pub notify_char_uuid: Option<String>,
    #[serde(default)]
    pub identity_char_uuid: Option<String>,
    #[serde(default)]
    pub scan_timeout_ms: Option<u64>,
    #[serde(default)]
    pub scan_duration_ms: Option<u64>,
    #[serde(default)]
    pub discovery_interval_ms: Option<u64>,
    #[serde(default)]
    pub discovery_interval_idle_ms: Option<u64>,
    #[serde(default)]
    pub advertising_refresh_interval_ms: Option<u64>,
    #[serde(default)]
    pub ble_connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_connections: Option<usize>,
    #[serde(default)]
    pub min_rssi_dbm: Option<i32>,
    #[serde(default)]
    pub enable_central: Option<bool>,
    #[serde(default)]
    pub enable_peripheral: Option<bool>,
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
            .filter(|iface| iface.enabled() && iface.kind == "tcp_client")
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
            .filter(|iface| iface.enabled() && iface.kind == "tcp_server")
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
