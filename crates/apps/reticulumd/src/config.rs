use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct DaemonConfig {
    pub interfaces: Vec<InterfaceConfig>,
}

#[derive(Debug, Deserialize)]
struct DaemonConfigRaw {
    #[serde(default)]
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
            iface.kind = iface.kind.trim().to_string();
            iface.validate(index).map_err(D::Error::custom)?;
        }
        Ok(Self { interfaces })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InterfaceConfig {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub baud_rate: Option<u32>,
    #[serde(default)]
    pub data_bits: Option<u8>,
    #[serde(default)]
    pub parity: Option<String>,
    #[serde(default)]
    pub stop_bits: Option<u8>,
    #[serde(default)]
    pub flow_control: Option<String>,
    #[serde(default)]
    pub mtu: Option<usize>,
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
}

impl InterfaceConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }

    pub fn settings_json(&self) -> Option<JsonValue> {
        let mut settings = JsonMap::new();
        match self.kind.as_str() {
            "serial" => {
                insert_opt_string(&mut settings, "device", self.device.as_ref());
                insert_opt_u64(&mut settings, "baud_rate", self.baud_rate.map(u64::from));
                insert_opt_u64(&mut settings, "data_bits", self.data_bits.map(u64::from));
                insert_opt_string(&mut settings, "parity", self.parity.as_ref());
                insert_opt_u64(&mut settings, "stop_bits", self.stop_bits.map(u64::from));
                insert_opt_string(&mut settings, "flow_control", self.flow_control.as_ref());
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
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
                insert_opt_u64(&mut settings, "connect_timeout_ms", self.connect_timeout_ms);
                insert_opt_u64(&mut settings, "mtu", self.mtu.map(|v| v as u64));
                insert_opt_u64(&mut settings, "reconnect_backoff_ms", self.reconnect_backoff_ms);
                insert_opt_u64(
                    &mut settings,
                    "max_reconnect_backoff_ms",
                    self.max_reconnect_backoff_ms,
                );
            }
            "lora" => {
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

    fn validate(&self, index: usize) -> Result<(), String> {
        let kind = self.kind.trim();
        if kind.is_empty() {
            return Err(format!("interfaces[{index}].type is required"));
        }
        match kind {
            "serial" => self.validate_serial(index),
            "ble_gatt" => self.validate_ble(index),
            "lora" => self.validate_lora(index),
            _ => Ok(()),
        }
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
            if !matches_normalized(parity, &["none", "even", "odd"]) {
                return Err(format!(
                    "interfaces[{index}].parity must be one of none, even, odd for serial"
                ));
            }
        }
        if let Some(flow_control) = self.flow_control.as_deref() {
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

    fn validate_lora(&self, index: usize) -> Result<(), String> {
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
        if let Some(spreading_factor) = self.spreading_factor {
            if !(5..=12).contains(&spreading_factor) {
                return Err(format!(
                    "interfaces[{index}].spreading_factor must be between 5 and 12 for lora"
                ));
            }
        }
        if let Some(coding_rate) = self.coding_rate.as_deref() {
            if !matches_normalized(coding_rate, &["4/5", "4/6", "4/7", "4/8"]) {
                return Err(format!(
                    "interfaces[{index}].coding_rate must be one of 4/5, 4/6, 4/7, 4/8 for lora"
                ));
            }
        }
        if let Some(bandwidth_hz) = self.bandwidth_hz {
            if !matches!(
                bandwidth_hz,
                7800 | 10400 | 15600 | 20800 | 31250 | 41700 | 62500 | 125000 | 250000 | 500000
            ) {
                return Err(format!(
                    "interfaces[{index}].bandwidth_hz is not a supported LoRa bandwidth"
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
        Ok(())
    }

    fn reject_unknown_new_kind_keys(&self, index: usize, kind: &str) -> Result<(), String> {
        if self.extra.is_empty() {
            return Ok(());
        }
        let mut unknown = self.extra.keys().cloned().collect::<Vec<_>>();
        unknown.sort();
        Err(format!(
            "interfaces[{index}] ({kind}) contains unknown settings key(s): {}",
            unknown.join(", ")
        ))
    }
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

fn insert_opt_u64(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::Number(value.into()));
    }
}

fn matches_normalized(value: &str, candidates: &[&str]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    candidates.iter().any(|candidate| normalized == *candidate)
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
