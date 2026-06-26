use rns_rpc::RNodeManagementBridge;
use rns_transport::hash::AddressHash;
use rns_transport::iface::lora::LoraRNodeManagementHandle;

use serde_json::{json, Value as JsonValue};

use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct RNodeManagementTarget {
    runtime_iface: String,
    name: String,
    handle: LoraRNodeManagementHandle,
}

pub(crate) struct DaemonRNodeManagementBinding {
    pub(crate) runtime_iface: AddressHash,
    pub(crate) name: String,
    pub(crate) handle: LoraRNodeManagementHandle,
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
        pattern: Option<u8>,
    ) -> Result<JsonValue, std::io::Error> {
        let target = self.resolve(iface)?;
        let normalized = command.trim().to_ascii_lowercase().replace('-', "_");
        match normalized.as_str() {
            "radio_state_query" | "query_radio_state" => {
                target.handle.try_query_radio_state().map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        format!("queue radio-state query failed: {err}"),
                    )
                })?;
                Ok(json!({
                    "queued": true,
                    "iface": target.runtime_iface,
                    "name": target.name,
                    "command": "radio_state_query",
                }))
            }
            "blink" => {
                let pattern = pattern.ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "blink pattern is required",
                    )
                })?;
                target.handle.try_blink(pattern).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        format!("queue blink failed: {err}"),
                    )
                })?;
                Ok(json!({
                    "queued": true,
                    "iface": target.runtime_iface,
                    "name": target.name,
                    "command": "blink",
                    "pattern": pattern,
                }))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported RNode management command '{command}'"),
            )),
        }
    }
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
            handle: iface.rnode_management_handle(),
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
            handle: iface.rnode_management_handle(),
        };
        (binding, iface)
    }

    #[test]
    fn bridge_dispatches_by_runtime_iface_and_name() {
        let runtime_iface = rns_transport::hash::AddressHash::new([0x31; 16]).to_string();
        let (binding, _iface) = live_binding(0x31, "rnode-main");
        let bridge = DaemonRNodeManagementBridge::new(vec![binding]);

        let by_runtime = bridge
            .dispatch_rnode_management(runtime_iface.as_str(), "radio_state_query", None)
            .expect("runtime iface dispatch");
        assert_eq!(by_runtime["queued"].as_bool(), Some(true));
        assert_eq!(by_runtime["command"].as_str(), Some("radio_state_query"));

        let by_name = bridge
            .dispatch_rnode_management("rnode-main", "blink", Some(3))
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
            .dispatch_rnode_management("duplicate", "blink", Some(1))
            .expect_err("duplicate name should be ambiguous");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("ambiguous"));
    }
}
