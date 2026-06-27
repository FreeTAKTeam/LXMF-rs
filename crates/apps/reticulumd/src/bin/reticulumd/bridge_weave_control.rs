use rns_rpc::WeaveDisplayControlBridge;
use rns_transport::hash::AddressHash;
use rns_transport::iface::weave::WeaveManagementHandle;

use serde_json::{json, Value as JsonValue};

use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct WeaveControlTarget {
    runtime_iface: String,
    name: String,
    handle: WeaveManagementHandle,
}

pub(crate) struct DaemonWeaveControlBinding {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) name: String,
    pub(crate) handle: WeaveManagementHandle,
}

pub(crate) struct DaemonWeaveDisplayControlBridge {
    by_runtime_iface: HashMap<String, WeaveControlTarget>,
    by_name: HashMap<String, WeaveControlTarget>,
    duplicate_names: HashSet<String>,
}

impl DaemonWeaveDisplayControlBridge {
    pub(crate) fn new(bindings: Vec<DaemonWeaveControlBinding>) -> Self {
        let mut by_runtime_iface = HashMap::new();
        let mut by_name = HashMap::new();
        let mut duplicate_names = HashSet::new();
        for binding in bindings {
            let runtime_iface = binding.runtime_iface.to_string();
            let target = WeaveControlTarget {
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

    fn resolve(&self, iface: &str) -> Result<&WeaveControlTarget, std::io::Error> {
        let selector = iface.trim();
        if let Some(target) = self.by_runtime_iface.get(selector) {
            return Ok(target);
        }
        if self.duplicate_names.contains(selector) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Weave interface name '{selector}' is ambiguous"),
            ));
        }
        self.by_name.get(selector).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Weave interface '{selector}' is not managed"),
            )
        })
    }
}

impl WeaveDisplayControlBridge for DaemonWeaveDisplayControlBridge {
    fn set_weave_remote_display(
        &self,
        iface: &str,
        enable: bool,
        remote_switch_id_hex: Option<&str>,
    ) -> Result<JsonValue, std::io::Error> {
        let target = self.resolve(iface)?;
        let explicit_switch = match remote_switch_id_hex {
            Some(value) => Some(parse_switch_id_hex(value)?),
            None => None,
        };
        let used_switch = target.handle.try_set_remote_display(explicit_switch, enable)?;
        Ok(json!({
            "queued": true,
            "name": target.name,
            "iface": target.runtime_iface,
            "enable": enable,
            "remote_switch_id_hex": hex_lower(&used_switch),
            "remote_switch_id_source": if explicit_switch.is_some() { "explicit" } else { "learned" },
        }))
    }
}

fn parse_switch_id_hex(value: &str) -> Result<[u8; 4], std::io::Error> {
    let trimmed = value.trim().strip_prefix("0x").unwrap_or(value.trim());
    if trimmed.len() != 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "remote_switch_id_hex must be exactly 4 bytes / 8 hex characters",
        ));
    }
    let mut out = [0_u8; 4];
    for (index, chunk) in trimmed.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "remote_switch_id_hex must be ASCII hex",
            )
        })?;
        out[index] = u8::from_str_radix(text, 16).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "remote_switch_id_hex must be a hex string",
            )
        })?;
    }
    Ok(out)
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
    use rns_transport::iface::{weave::WeaveInterface, InterfaceManager};
    use std::sync::Arc;

    fn live_binding(runtime_byte: u8, name: &str) -> (DaemonWeaveControlBinding, WeaveInterface) {
        let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
        let iface = WeaveInterface::new("test", manager);
        let binding = DaemonWeaveControlBinding {
            runtime_iface: AddressHash::new([runtime_byte; 16]),
            name: name.to_string(),
            handle: iface.weave_management_handle(),
        };
        (binding, iface)
    }

    fn binding(runtime_byte: u8, name: &str) -> DaemonWeaveControlBinding {
        live_binding(runtime_byte, name).0
    }

    #[test]
    fn bridge_dispatches_by_runtime_iface_and_name() {
        let runtime_iface = AddressHash::new([0x31; 16]).to_string();
        let (binding, _iface) = live_binding(0x31, "weave-main");
        let bridge = DaemonWeaveDisplayControlBridge::new(vec![binding]);

        let by_runtime = bridge
            .set_weave_remote_display(runtime_iface.as_str(), true, Some("10203040"))
            .expect("runtime iface dispatch");
        assert_eq!(by_runtime["queued"].as_bool(), Some(true));
        assert_eq!(by_runtime["enable"].as_bool(), Some(true));
        assert_eq!(by_runtime["remote_switch_id_hex"].as_str(), Some("10203040"));
        assert_eq!(by_runtime["remote_switch_id_source"].as_str(), Some("explicit"));

        let by_name = bridge
            .set_weave_remote_display("weave-main", false, Some("50607080"))
            .expect("name dispatch");
        assert_eq!(by_name["queued"].as_bool(), Some(true));
        assert_eq!(by_name["enable"].as_bool(), Some(false));
        assert_eq!(by_name["remote_switch_id_hex"].as_str(), Some("50607080"));
    }

    #[test]
    fn bridge_rejects_ambiguous_names() {
        let bridge = DaemonWeaveDisplayControlBridge::new(vec![
            binding(0x41, "duplicate"),
            binding(0x42, "duplicate"),
        ]);

        let err = bridge
            .set_weave_remote_display("duplicate", true, Some("10203040"))
            .expect_err("duplicate name should be ambiguous");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn bridge_rejects_missing_switch_before_discovery() {
        let (binding, _iface) = live_binding(0x51, "weave-controls");
        let bridge = DaemonWeaveDisplayControlBridge::new(vec![binding]);

        let err = bridge
            .set_weave_remote_display("weave-controls", true, None)
            .expect_err("remote switch should be required before discovery");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("remote_switch_id is required"));
    }

    #[test]
    fn bridge_rejects_malformed_switch_hex() {
        let (binding, _iface) = live_binding(0x61, "weave-controls");
        let bridge = DaemonWeaveDisplayControlBridge::new(vec![binding]);

        let err = bridge
            .set_weave_remote_display("weave-controls", true, Some("123"))
            .expect_err("short switch id should be rejected");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("8 hex"));
    }
}
