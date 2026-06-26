use rns_rpc::RNodeManagementBridge;
use rns_transport::hash::AddressHash;
use rns_transport::iface::lora::{LoraConfig, LoraRNodeManagementHandle};
#[cfg(feature = "rnode-ble")]
use rns_transport::iface::rnode_ble::RnodeBleManagementHandle;
use rns_transport::iface::rnode_multi::RNodeMultiManagementHandle;

use serde_json::{json, Value as JsonValue};

use std::collections::{HashMap, HashSet};

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
        }
        if let (Some(result), Some(echoed)) = (result.as_object_mut(), echoed.as_object()) {
            result.extend(echoed.clone());
        }
        Ok(result)
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
}
