use rns_rpc::RNodeManagementBridge;
use rns_transport::hash::AddressHash;
use rns_transport::iface::lora::{LoraConfig, LoraRNodeManagementHandle, RNodeProbeStatus};
#[cfg(feature = "rnode-ble")]
use rns_transport::iface::rnode_ble::RnodeBleManagementHandle;
use rns_transport::iface::rnode_multi::RNodeMultiManagementHandle;

use serde_json::{json, Value as JsonValue};

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;

#[derive(Clone)]
struct RNodeManagementTarget {
    runtime_iface: String,
    name: String,
    handle: DaemonRNodeManagementHandle,
}

#[derive(Clone)]
pub(crate) enum DaemonRNodeManagementHandle {
    Lora(LoraRNodeManagementHandle),
    #[cfg(feature = "rnode-ble")]
    RnodeBle(RnodeBleManagementHandle),
    RNodeMulti {
        handle: RNodeMultiManagementHandle,
        allowed_vports: Vec<u8>,
    },
}

impl DaemonRNodeManagementHandle {
    fn selected_vport(&self, params: &JsonValue) -> Result<Option<u8>, std::io::Error> {
        match self {
            Self::Lora(_) => Ok(None),
            #[cfg(feature = "rnode-ble")]
            Self::RnodeBle(_) => Ok(None),
            Self::RNodeMulti { allowed_vports, .. } => {
                let vport = param_u8(params, &["vport"])?;
                if allowed_vports.contains(&vport) {
                    Ok(Some(vport))
                } else {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("vport {vport} is not configured for this RNodeMulti interface"),
                    ))
                }
            }
        }
    }

    fn try_dispatch_frame(&self, vport: Option<u8>, frame: Vec<u8>) -> Result<(), String> {
        match self {
            Self::Lora(handle) => handle.try_dispatch_frame(frame).map_err(|err| err.to_string()),
            #[cfg(feature = "rnode-ble")]
            Self::RnodeBle(handle) => {
                handle.try_dispatch_frame(frame).map_err(|err| err.to_string())
            }
            Self::RNodeMulti { handle, .. } => {
                let vport = vport.expect("RNodeMulti vport should be validated before dispatch");
                handle.try_dispatch_frame(vport, frame).map_err(|err| err.to_string())
            }
        }
    }
}

pub(crate) struct DaemonRNodeManagementBinding {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) name: String,
    pub(crate) handle: DaemonRNodeManagementHandle,
}

pub(crate) struct DaemonRNodeManagementBridge {
    by_runtime_iface: HashMap<String, RNodeManagementTarget>,
    by_name: HashMap<String, RNodeManagementTarget>,
    duplicate_names: HashSet<String>,
}

impl DaemonRNodeManagementBridge {
    pub(crate) fn new(bindings: Vec<DaemonRNodeManagementBinding>) -> Self {
        let mut by_runtime_iface = HashMap::new();
        let mut by_name = HashMap::new();
        let mut duplicate_names = HashSet::new();
        for binding in bindings {
            let runtime_iface = binding.runtime_iface.to_string();
            let target = RNodeManagementTarget {
                runtime_iface: runtime_iface.clone(),
                name: binding.name,
                handle: binding.handle,
            };
            by_runtime_iface.insert(runtime_iface, target.clone());
            let name_key = target.name.trim().to_string();
            if !name_key.is_empty() && by_name.insert(name_key.clone(), target).is_some() {
                duplicate_names.insert(name_key);
            }
        }
        for name in &duplicate_names {
            by_name.remove(name);
        }
        Self { by_runtime_iface, by_name, duplicate_names }
    }

    fn resolve(&self, iface: &str) -> Result<&RNodeManagementTarget, std::io::Error> {
        let selector = iface.trim();
        if let Some(target) = self.by_runtime_iface.get(selector) {
            return Ok(target);
        }
        if self.duplicate_names.contains(selector) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("RNode interface name '{selector}' is ambiguous"),
            ));
        }
        self.by_name.get(selector).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("RNode interface '{selector}' is not managed"),
            )
        })
    }
}

impl RNodeManagementBridge for DaemonRNodeManagementBridge {
    fn dispatch_rnode_management(
        &self,
        iface: &str,
        command: &str,
        params: &JsonValue,
    ) -> Result<JsonValue, std::io::Error> {
        let target = self.resolve(iface)?;
        let normalized = command.trim().to_ascii_lowercase().replace('-', "_");
        let (canonical, frame, echoed) = match normalized.as_str() {
            "radio_state_query" | "query_radio_state" => {
                ("radio_state_query", LoraConfig::radio_state_query_frame(), json!({}))
            }
            "blink" => {
                let pattern = param_u8(params, &["pattern"])?;
                ("blink", LoraConfig::blink_frame(pattern), json!({ "pattern": pattern }))
            }
            "config_read" | "read_config" => {
                ("config_read", LoraConfig::config_read_frame(), json!({}))
            }
            "rom_read" | "read_rom" => ("rom_read", LoraConfig::rom_read_frame(), json!({})),
            "display_intensity" | "set_display_intensity" => {
                let intensity = param_u8(params, &["intensity"])?;
                (
                    "display_intensity",
                    LoraConfig::display_intensity_frame(intensity),
                    json!({ "intensity": intensity }),
                )
            }
            "display_blanking" | "set_display_blanking" => {
                let blanking_timeout = param_u8(params, &["blanking_timeout", "timeout"])?;
                (
                    "display_blanking",
                    LoraConfig::display_blanking_frame(blanking_timeout),
                    json!({ "blanking_timeout": blanking_timeout }),
                )
            }
            "display_rotation" | "set_display_rotation" => {
                let rotation = param_u8(params, &["rotation"])?;
                (
                    "display_rotation",
                    LoraConfig::display_rotation_frame(rotation),
                    json!({ "rotation": rotation }),
                )
            }
            "display_recondition" | "recondition_display" => {
                ("display_recondition", LoraConfig::display_recondition_frame(), json!({}))
            }
            "display_address" | "set_display_address" => {
                let address = param_u8(params, &["address"])?;
                (
                    "display_address",
                    LoraConfig::display_address_frame(address),
                    json!({ "address": address }),
                )
            }
            "neopixel_intensity" | "set_neopixel_intensity" => {
                let intensity = param_u8(params, &["intensity"])?;
                (
                    "neopixel_intensity",
                    LoraConfig::neopixel_intensity_frame(intensity),
                    json!({ "intensity": intensity }),
                )
            }
            "disable_interference_avoidance" => {
                let disabled = param_bool(params, &["disabled"])?;
                (
                    "disable_interference_avoidance",
                    LoraConfig::disable_interference_avoidance_frame(disabled),
                    json!({ "disabled": disabled }),
                )
            }
            "enable_interference_avoidance" => (
                "disable_interference_avoidance",
                LoraConfig::disable_interference_avoidance_frame(false),
                json!({ "disabled": false }),
            ),
            "bluetooth_enable" | "enable_bluetooth" => {
                require_persistent_confirm(params, "bluetooth_enable")?;
                ("bluetooth_enable", LoraConfig::bluetooth_enable_frame(), json!({}))
            }
            "bluetooth_disable" | "disable_bluetooth" => {
                require_persistent_confirm(params, "bluetooth_disable")?;
                ("bluetooth_disable", LoraConfig::bluetooth_disable_frame(), json!({}))
            }
            "bluetooth_pair" | "pair_bluetooth" => {
                require_persistent_confirm(params, "bluetooth_pair")?;
                ("bluetooth_pair", LoraConfig::bluetooth_pair_frame(), json!({}))
            }
            "config_save" | "save_config" => {
                require_persistent_confirm(params, "config_save")?;
                ("config_save", LoraConfig::config_save_frame(), json!({}))
            }
            "config_delete" | "delete_config" => {
                require_destructive_confirm(params, "config_delete")?;
                ("config_delete", LoraConfig::config_delete_frame(), json!({}))
            }
            "rom_wipe" | "wipe_rom" => {
                require_destructive_confirm(params, "rom_wipe")?;
                ("rom_wipe", LoraConfig::rom_wipe_frame(), json!({}))
            }
            "rom_write" | "write_rom" => {
                require_destructive_confirm(params, "rom_write")?;
                let address = param_u8(params, &["address", "addr"])?;
                let byte = param_u8(params, &["byte", "value"])?;
                (
                    "rom_write",
                    LoraConfig::rom_write_frame(address, byte),
                    json!({ "address": address, "byte": byte }),
                )
            }
            "hard_reset" | "reset" => {
                require_destructive_confirm(params, "hard_reset")?;
                ("hard_reset", RNodeProbeStatus::hard_reset_frame(), json!({}))
            }
            "firmware_update_indicator" | "set_firmware_update_indicator" | "firmware_update" => {
                require_persistent_confirm(params, "firmware_update_indicator")?;
                (
                    "firmware_update_indicator",
                    LoraConfig::firmware_update_indicator_frame(),
                    json!({}),
                )
            }
            "firmware_hash" | "set_firmware_hash" => {
                require_persistent_confirm(params, "firmware_hash")?;
                let hash = param_hex_bytes(params, &["hash_hex", "hash"])?;
                if hash.len() > 64 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "firmware hash must be at most 64 bytes",
                    ));
                }
                (
                    "firmware_hash",
                    LoraConfig::firmware_hash_frame(&hash),
                    json!({ "hash_hex": hex_lower(&hash) }),
                )
            }
            "wifi_mode" | "set_wifi_mode" => {
                require_persistent_confirm(params, "wifi_mode")?;
                let mode = param_u8(params, &["mode"])?;
                ("wifi_mode", LoraConfig::wifi_mode_frame(mode), json!({ "mode": mode }))
            }
            "wifi_channel" | "set_wifi_channel" => {
                require_persistent_confirm(params, "wifi_channel")?;
                let channel = param_u8(params, &["channel"])?;
                let frame = LoraConfig::wifi_channel_frame(channel).map_err(invalid_input)?;
                ("wifi_channel", frame, json!({ "channel": channel }))
            }
            "wifi_ip" | "set_wifi_ip" => {
                require_persistent_confirm(params, "wifi_ip")?;
                let ip = param_optional_ipv4(params, &["ip", "address"])?;
                (
                    "wifi_ip",
                    LoraConfig::wifi_ip_frame(ip),
                    json!({ "ip": ip.map(|ip| ip.to_string()) }),
                )
            }
            "clear_wifi_ip" => {
                require_persistent_confirm(params, "wifi_ip")?;
                ("wifi_ip", LoraConfig::wifi_ip_frame(None), json!({ "ip": JsonValue::Null }))
            }
            "wifi_netmask" | "set_wifi_netmask" => {
                require_persistent_confirm(params, "wifi_netmask")?;
                let netmask = param_optional_ipv4(params, &["netmask", "mask"])?;
                (
                    "wifi_netmask",
                    LoraConfig::wifi_netmask_frame(netmask),
                    json!({ "netmask": netmask.map(|netmask| netmask.to_string()) }),
                )
            }
            "clear_wifi_netmask" => {
                require_persistent_confirm(params, "wifi_netmask")?;
                (
                    "wifi_netmask",
                    LoraConfig::wifi_netmask_frame(None),
                    json!({ "netmask": JsonValue::Null }),
                )
            }
            "wifi_ssid" | "set_wifi_ssid" => {
                require_persistent_confirm(params, "wifi_ssid")?;
                let ssid = param_optional_string(params, &["ssid"])?;
                let frame = LoraConfig::wifi_ssid_frame(ssid.as_deref()).map_err(invalid_input)?;
                ("wifi_ssid", frame, json!({ "ssid": ssid }))
            }
            "clear_wifi_ssid" => {
                require_persistent_confirm(params, "wifi_ssid")?;
                let frame = LoraConfig::wifi_ssid_frame(None).map_err(invalid_input)?;
                ("wifi_ssid", frame, json!({ "ssid": JsonValue::Null }))
            }
            "wifi_psk" | "set_wifi_psk" => {
                require_persistent_confirm(params, "wifi_psk")?;
                let psk = param_optional_string(params, &["psk", "password"])?;
                let frame = LoraConfig::wifi_psk_frame(psk.as_deref()).map_err(invalid_input)?;
                ("wifi_psk", frame, json!({ "psk_set": psk.is_some() }))
            }
            "clear_wifi_psk" => {
                require_persistent_confirm(params, "wifi_psk")?;
                let frame = LoraConfig::wifi_psk_frame(None).map_err(invalid_input)?;
                ("wifi_psk", frame, json!({ "psk_set": false }))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported RNode management command '{command}'"),
            ))?,
        };
        let vport = target.handle.selected_vport(params)?;
        target.handle.try_dispatch_frame(vport, frame).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                format!("queue {canonical} failed: {err}"),
            )
        })?;
        let mut result = json!({
            "queued": true,
            "name": target.name,
            "command": canonical,
            "iface": target.runtime_iface,
        });
        if let Some(result) = result.as_object_mut() {
            if let Some(vport) = vport {
                result.insert("vport".to_string(), json!(vport));
            }
            if is_persistent_command(canonical) {
                result.insert("confirmation".to_string(), json!("persistent"));
            }
            if is_destructive_command(canonical) {
                result.insert("confirmation".to_string(), json!("destructive"));
            }
        }
        if let (Some(result), Some(echoed)) = (result.as_object_mut(), echoed.as_object()) {
            result.extend(echoed.clone());
        }
        Ok(result)
    }
}

fn is_persistent_command(canonical: &str) -> bool {
    matches!(
        canonical,
        "bluetooth_enable"
            | "bluetooth_disable"
            | "bluetooth_pair"
            | "config_save"
            | "firmware_update_indicator"
            | "firmware_hash"
            | "wifi_mode"
            | "wifi_channel"
            | "wifi_ip"
            | "wifi_netmask"
            | "wifi_ssid"
            | "wifi_psk"
    )
}

fn is_destructive_command(canonical: &str) -> bool {
    matches!(canonical, "config_delete" | "rom_wipe" | "rom_write" | "hard_reset")
}

fn require_persistent_confirm(params: &JsonValue, canonical: &str) -> Result<(), std::io::Error> {
    match params.get("confirm_persistent") {
        Some(value) if value.as_bool() == Some(true) => Ok(()),
        Some(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{canonical} requires confirm_persistent=true"),
        )),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{canonical} requires confirm_persistent=true"),
        )),
    }
}

fn require_destructive_confirm(params: &JsonValue, canonical: &str) -> Result<(), std::io::Error> {
    match params.get("confirm_destructive") {
        Some(value) if value.as_bool() == Some(true) => {}
        Some(_) | None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{canonical} requires confirm_destructive=true"),
            ));
        }
    }
    match params.get("confirm_command").and_then(JsonValue::as_str) {
        Some(value) if value == canonical => Ok(()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{canonical} requires confirm_command=\"{canonical}\""),
        )),
    }
}

fn param_u8(params: &JsonValue, keys: &[&str]) -> Result<u8, std::io::Error> {
    for key in keys {
        if let Some(value) = params.get(*key) {
            let Some(value) = value.as_u64() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{key} must be an integer between 0 and 255"),
                ));
            };
            return u8::try_from(value).map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{key} must be an integer between 0 and 255"),
                )
            });
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} is required", keys[0])))
}

fn param_optional_string(
    params: &JsonValue,
    keys: &[&str],
) -> Result<Option<String>, std::io::Error> {
    for key in keys {
        if let Some(value) = params.get(*key) {
            if value.is_null() {
                return Ok(None);
            }
            let Some(value) = value.as_str() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{key} must be a string or null"),
                ));
            };
            return Ok(Some(value.to_string()));
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} is required", keys[0])))
}

fn param_optional_ipv4(
    params: &JsonValue,
    keys: &[&str],
) -> Result<Option<Ipv4Addr>, std::io::Error> {
    let Some(value) = param_optional_string(params, keys)? else {
        return Ok(None);
    };
    value.parse::<Ipv4Addr>().map(Some).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} must be an IPv4 address string or null", keys[0]),
        )
    })
}

fn param_hex_bytes(params: &JsonValue, keys: &[&str]) -> Result<Vec<u8>, std::io::Error> {
    let value = param_optional_string(params, keys)?.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} is required", keys[0]))
    })?;
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() % 2 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} must be a non-empty even-length hex string", keys[0]),
        ));
    }
    let mut bytes = Vec::with_capacity(trimmed.len() / 2);
    for index in (0..trimmed.len()).step_by(2) {
        let byte = u8::from_str_radix(&trimmed[index..index + 2], 16).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} must be a hex string", keys[0]),
            )
        })?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn param_bool(params: &JsonValue, keys: &[&str]) -> Result<bool, std::io::Error> {
    for key in keys {
        if let Some(value) = params.get(*key) {
            return value.as_bool().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{key} must be a boolean"),
                )
            });
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{} is required", keys[0])))
}

fn invalid_input(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(runtime_byte: u8, name: &str) -> DaemonRNodeManagementBinding {
        let iface = rns_transport::iface::lora::LoraInterface::new(
            "COM9",
            115_200,
            rns_transport::iface::lora::LoraConfig::us915_default(),
        );
        DaemonRNodeManagementBinding {
            runtime_iface: rns_transport::hash::AddressHash::new([runtime_byte; 16]),
            name: name.to_string(),
            handle: DaemonRNodeManagementHandle::Lora(iface.rnode_management_handle()),
        }
    }

    fn live_binding(
        runtime_byte: u8,
        name: &str,
    ) -> (DaemonRNodeManagementBinding, rns_transport::iface::lora::LoraInterface) {
        let iface = rns_transport::iface::lora::LoraInterface::new(
            "COM9",
            115_200,
            rns_transport::iface::lora::LoraConfig::us915_default(),
        );
        let binding = DaemonRNodeManagementBinding {
            runtime_iface: rns_transport::hash::AddressHash::new([runtime_byte; 16]),
            name: name.to_string(),
            handle: DaemonRNodeManagementHandle::Lora(iface.rnode_management_handle()),
        };
        (binding, iface)
    }

    #[test]
    fn bridge_dispatches_by_runtime_iface_and_name() {
        let runtime_iface = rns_transport::hash::AddressHash::new([0x31; 16]).to_string();
        let (binding, _iface) = live_binding(0x31, "rnode-main");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        let by_runtime = bridge
            .dispatch_rnode_management(runtime_iface.as_str(), "radio_state_query", &json!({}))
            .expect("runtime iface dispatch");
        assert_eq!(by_runtime["queued"].as_bool(), Some(true));
        assert_eq!(by_runtime["command"].as_str(), Some("radio_state_query"));

        let by_name = bridge
            .dispatch_rnode_management("rnode-main", "blink", &json!({ "pattern": 3 }))
            .expect("name dispatch");
        assert_eq!(by_name["queued"].as_bool(), Some(true));
        assert_eq!(by_name["command"].as_str(), Some("blink"));
        assert_eq!(by_name["pattern"].as_u64(), Some(3));
    }

    #[test]
    fn bridge_rejects_ambiguous_names() {
        let bridge = DaemonRNodeManagementBridge::new(vec![
            binding(0x41, "duplicate"),
            binding(0x42, "duplicate"),
        ]);

        let err = bridge
            .dispatch_rnode_management("duplicate", "blink", &json!({ "pattern": 1 }))
            .expect_err("duplicate name should be ambiguous");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn bridge_dispatches_safe_management_frame_commands() {
        let (binding, _iface) = live_binding(0x51, "rnode-controls");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        for (command, params, field, expected) in [
            ("read-config", json!({}), None, None),
            ("read-rom", json!({}), None, None),
            ("set-display-intensity", json!({ "intensity": 8 }), Some("intensity"), Some(8)),
            ("set-display-blanking", json!({ "timeout": 12 }), Some("blanking_timeout"), Some(12)),
            ("set-display-rotation", json!({ "rotation": 2 }), Some("rotation"), Some(2)),
            ("recondition-display", json!({}), None, None),
            ("set-display-address", json!({ "address": 60 }), Some("address"), Some(60)),
            ("set-neopixel-intensity", json!({ "intensity": 4 }), Some("intensity"), Some(4)),
        ] {
            let result = bridge
                .dispatch_rnode_management("rnode-controls", command, &params)
                .expect("management command should queue");
            assert_eq!(result["queued"].as_bool(), Some(true), "{command}");
            if let (Some(field), Some(expected)) = (field, expected) {
                assert_eq!(result[field].as_u64(), Some(expected), "{command}");
            }
        }

        let result = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "disable-interference-avoidance",
                &json!({ "disabled": true }),
            )
            .expect("disable ia should queue");
        assert_eq!(result["command"].as_str(), Some("disable_interference_avoidance"));
        assert_eq!(result["disabled"].as_bool(), Some(true));

        let result = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "enable-interference-avoidance",
                &json!({}),
            )
            .expect("enable ia should queue");
        assert_eq!(result["disabled"].as_bool(), Some(false));
    }

    #[test]
    fn bridge_rejects_missing_required_management_params() {
        let (binding, _iface) = live_binding(0x52, "rnode-controls");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        let err = bridge
            .dispatch_rnode_management("rnode-controls", "set-display-intensity", &json!({}))
            .expect_err("intensity is required");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("intensity is required"));
    }

    #[test]
    fn bridge_dispatches_persistent_management_commands_with_confirmation() {
        let (binding, _iface) = live_binding(0x53, "rnode-controls");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        for (command, params, field, expected) in [
            ("enable-bluetooth", json!({ "confirm_persistent": true }), None, None),
            ("disable-bluetooth", json!({ "confirm_persistent": true }), None, None),
            ("pair-bluetooth", json!({ "confirm_persistent": true }), None, None),
            ("save-config", json!({ "confirm_persistent": true }), None, None),
            ("firmware-update", json!({ "confirm_persistent": true }), None, None),
            (
                "firmware-hash",
                json!({ "confirm_persistent": true, "hash_hex": "a1b2" }),
                Some("hash_hex"),
                None,
            ),
            (
                "set-wifi-mode",
                json!({ "confirm_persistent": true, "mode": 1 }),
                Some("mode"),
                Some(1_u64),
            ),
            (
                "set-wifi-channel",
                json!({ "confirm_persistent": true, "channel": 11 }),
                Some("channel"),
                Some(11_u64),
            ),
            (
                "set-wifi-ip",
                json!({ "confirm_persistent": true, "ip": "192.0.2.10" }),
                Some("ip"),
                None,
            ),
            ("clear-wifi-netmask", json!({ "confirm_persistent": true }), Some("netmask"), None),
            (
                "set-wifi-ssid",
                json!({ "confirm_persistent": true, "ssid": "mesh" }),
                Some("ssid"),
                None,
            ),
            (
                "set-wifi-psk",
                json!({ "confirm_persistent": true, "psk": "abcdefgh" }),
                Some("psk_set"),
                None,
            ),
        ] {
            let result = bridge
                .dispatch_rnode_management("rnode-controls", command, &params)
                .expect("confirmed management command should queue");
            assert_eq!(result["queued"].as_bool(), Some(true), "{command}");
            assert_eq!(result["confirmation"].as_str(), Some("persistent"), "{command}");
            if let (Some(field), Some(expected)) = (field, expected) {
                assert_eq!(result[field].as_u64(), Some(expected), "{command}");
            }
        }

        let result = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "firmware-hash",
                &json!({ "confirm_persistent": true, "hash_hex": "A1B2" }),
            )
            .expect("firmware hash should queue");
        assert_eq!(result["hash_hex"].as_str(), Some("a1b2"));

        let result = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "clear-wifi-ip",
                &json!({ "confirm_persistent": true }),
            )
            .expect("wifi ip clear should queue");
        assert!(result["ip"].is_null());

        let result = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "clear-wifi-psk",
                &json!({ "confirm_persistent": true }),
            )
            .expect("wifi psk clear should queue");
        assert_eq!(result["psk_set"].as_bool(), Some(false));
    }

    #[test]
    fn bridge_dispatches_destructive_management_commands_with_exact_confirmation() {
        let (binding, _iface) = live_binding(0x54, "rnode-controls");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        for (command, params) in [
            (
                "delete-config",
                json!({ "confirm_destructive": true, "confirm_command": "config_delete" }),
            ),
            ("wipe-rom", json!({ "confirm_destructive": true, "confirm_command": "rom_wipe" })),
            (
                "write-rom",
                json!({
                    "confirm_destructive": true,
                    "confirm_command": "rom_write",
                    "address": 9,
                    "byte": 42
                }),
            ),
            ("hard-reset", json!({ "confirm_destructive": true, "confirm_command": "hard_reset" })),
        ] {
            let result = bridge
                .dispatch_rnode_management("rnode-controls", command, &params)
                .expect("destructive management command should queue");
            assert_eq!(result["queued"].as_bool(), Some(true), "{command}");
            assert_eq!(result["confirmation"].as_str(), Some("destructive"), "{command}");
        }
    }

    #[test]
    fn bridge_rejects_unconfirmed_or_invalid_guarded_management_commands() {
        let (binding, _iface) = live_binding(0x54, "rnode-controls");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        let err = bridge
            .dispatch_rnode_management("rnode-controls", "save-config", &json!({}))
            .expect_err("confirmation is required");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("confirm_persistent=true"));

        let err = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "wipe-rom",
                &json!({ "confirm_destructive": true, "confirm_command": "rom_write" }),
            )
            .expect_err("destructive command name must match");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("confirm_command=\"rom_wipe\""));

        let err = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "set-wifi-channel",
                &json!({ "confirm_persistent": true, "channel": 15 }),
            )
            .expect_err("wifi channel is bounded");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("between 1 and 14"));

        let err = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "firmware-hash",
                &json!({ "confirm_persistent": true, "hash_hex": "abc" }),
            )
            .expect_err("firmware hash is even-length hex");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("even-length hex"));

        let too_long_hash = "aa".repeat(65);
        let err = bridge
            .dispatch_rnode_management(
                "rnode-controls",
                "firmware-hash",
                &json!({ "confirm_persistent": true, "hash_hex": too_long_hash }),
            )
            .expect_err("firmware hash is length-capped");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("at most 64 bytes"));
    }

    #[test]
    fn bridge_dispatches_rnode_multi_management_by_parent_selector_and_vport() {
        let runtime_iface = rns_transport::hash::AddressHash::new([0x61; 16]);
        let iface_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            rns_transport::iface::InterfaceManager::new(8),
        ));
        let iface =
            rns_transport::iface::rnode_multi::RNodeMultiInterface::new("COM9", iface_manager)
                .with_subinterfaces(vec![
                    rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig {
                        name: "rnode-child".to_string(),
                        vport: 2,
                        config: rns_transport::iface::lora::LoraConfig::us915_default(),
                        outgoing: true,
                    },
                ]);
        let bridge = DaemonRNodeManagementBridge::new(vec![DaemonRNodeManagementBinding {
            runtime_iface,
            name: "rnode-main".to_string(),
            handle: DaemonRNodeManagementHandle::RNodeMulti {
                handle: iface.rnode_management_handle(),
                allowed_vports: vec![2, 3],
            },
        }]);

        let result = bridge
            .dispatch_rnode_management("rnode-main", "blink", &json!({ "vport": 2, "pattern": 3 }))
            .expect("rnode multi management should queue by parent selector and vport");

        assert_eq!(result["queued"].as_bool(), Some(true));
        assert_eq!(result["name"].as_str(), Some("rnode-main"));
        assert_eq!(result["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
        assert_eq!(result["vport"].as_u64(), Some(2));
        assert_eq!(result["pattern"].as_u64(), Some(3));
    }

    #[test]
    fn bridge_rejects_rnode_multi_management_without_or_unknown_vport() {
        let iface_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            rns_transport::iface::InterfaceManager::new(8),
        ));
        let iface =
            rns_transport::iface::rnode_multi::RNodeMultiInterface::new("COM9", iface_manager)
                .with_subinterfaces(vec![
                    rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig {
                        name: "rnode-child".to_string(),
                        vport: 2,
                        config: rns_transport::iface::lora::LoraConfig::us915_default(),
                        outgoing: true,
                    },
                ]);
        let bridge = DaemonRNodeManagementBridge::new(vec![DaemonRNodeManagementBinding {
            runtime_iface: rns_transport::hash::AddressHash::new([0x62; 16]),
            name: "rnode-main".to_string(),
            handle: DaemonRNodeManagementHandle::RNodeMulti {
                handle: iface.rnode_management_handle(),
                allowed_vports: vec![2],
            },
        }]);

        let missing = bridge
            .dispatch_rnode_management("rnode-main", "blink", &json!({ "pattern": 3 }))
            .expect_err("vport is required");
        assert_eq!(missing.kind(), std::io::ErrorKind::InvalidInput);
        assert!(missing.to_string().contains("vport is required"));

        let unknown = bridge
            .dispatch_rnode_management("rnode-main", "blink", &json!({ "vport": 3, "pattern": 3 }))
            .expect_err("unknown vport is rejected");
        assert_eq!(unknown.kind(), std::io::ErrorKind::InvalidInput);
        assert!(unknown.to_string().contains("vport 3 is not configured"));
    }

    #[test]
    fn bridge_applies_rnode_multi_vport_to_guarded_management_commands() {
        let iface_manager = std::sync::Arc::new(tokio::sync::Mutex::new(
            rns_transport::iface::InterfaceManager::new(8),
        ));
        let iface =
            rns_transport::iface::rnode_multi::RNodeMultiInterface::new("COM9", iface_manager)
                .with_subinterfaces(vec![
                    rns_transport::iface::rnode_multi::RNodeMultiSubInterfaceConfig {
                        name: "rnode-child".to_string(),
                        vport: 2,
                        config: rns_transport::iface::lora::LoraConfig::us915_default(),
                        outgoing: true,
                    },
                ]);
        let bridge = DaemonRNodeManagementBridge::new(vec![DaemonRNodeManagementBinding {
            runtime_iface: rns_transport::hash::AddressHash::new([0x63; 16]),
            name: "rnode-main".to_string(),
            handle: DaemonRNodeManagementHandle::RNodeMulti {
                handle: iface.rnode_management_handle(),
                allowed_vports: vec![2],
            },
        }]);

        let result = bridge
            .dispatch_rnode_management(
                "rnode-main",
                "save-config",
                &json!({ "vport": 2, "confirm_persistent": true }),
            )
            .expect("guarded rnode multi management should queue");

        assert_eq!(result["queued"].as_bool(), Some(true));
        assert_eq!(result["command"].as_str(), Some("config_save"));
        assert_eq!(result["vport"].as_u64(), Some(2));
        assert_eq!(result["confirmation"].as_str(), Some("persistent"));
    }

    #[cfg(feature = "rnode-ble")]
    #[test]
    fn bridge_dispatches_native_rnode_ble_management_commands() {
        let runtime_iface = rns_transport::hash::AddressHash::new([0x71; 16]);
        let iface = rns_transport::iface::rnode_ble::NativeRnodeBleKissInterface::new(
            "rnode-ble",
            rns_transport::iface::rnode_ble::NativeRnodeBleSettings::for_peripheral("RNode BLE"),
            rns_transport::iface::rnode_ble::RnodeBleKissConfig::default(),
        );
        let bridge = DaemonRNodeManagementBridge::new(vec![DaemonRNodeManagementBinding {
            runtime_iface,
            name: "rnode-ble-main".to_string(),
            handle: DaemonRNodeManagementHandle::RnodeBle(iface.rnode_management_handle()),
        }]);

        for (command, params, canonical, confirmation) in [
            ("query-radio-state", json!({}), "radio_state_query", None),
            ("blink", json!({ "pattern": 5 }), "blink", None),
            (
                "enable-bluetooth",
                json!({ "confirm_persistent": true }),
                "bluetooth_enable",
                Some("persistent"),
            ),
            (
                "set-wifi-channel",
                json!({ "channel": 11, "confirm_persistent": true }),
                "wifi_channel",
                Some("persistent"),
            ),
            (
                "wipe-rom",
                json!({ "confirm_destructive": true, "confirm_command": "rom_wipe" }),
                "rom_wipe",
                Some("destructive"),
            ),
        ] {
            let result = bridge
                .dispatch_rnode_management("rnode-ble-main", command, &params)
                .expect("BLE management command should queue");
            assert_eq!(result["queued"].as_bool(), Some(true), "{command}");
            assert_eq!(result["command"].as_str(), Some(canonical), "{command}");
            assert_eq!(result["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
            if let Some(confirmation) = confirmation {
                assert_eq!(result["confirmation"].as_str(), Some(confirmation), "{command}");
            }
        }

        let err = bridge
            .dispatch_rnode_management("rnode-ble-main", "save-config", &json!({}))
            .expect_err("BLE persistent command should require confirmation");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("confirm_persistent=true"));
    }
}
