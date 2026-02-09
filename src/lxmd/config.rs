use crate::error::LxmfError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LxmdConfig {
    pub propagation_node: bool,
    pub announce_interval_secs: u64,
    pub service_tick_interval_secs: u64,
    pub propagation_target_cost: u32,
    pub on_inbound: Option<String>,
    pub rnsconfig: Option<String>,
    pub storage_path: Option<String>,
    pub identity_path: Option<String>,
}

impl Default for LxmdConfig {
    fn default() -> Self {
        Self {
            propagation_node: false,
            announce_interval_secs: 3600,
            service_tick_interval_secs: 1,
            propagation_target_cost: crate::constants::PROPAGATION_COST,
            on_inbound: None,
            rnsconfig: None,
            storage_path: None,
            identity_path: None,
        }
    }
}

impl LxmdConfig {
    pub fn load_from_path(path: &Path) -> Result<Self, LxmfError> {
        let raw = std::fs::read_to_string(path).map_err(|e| LxmfError::Io(e.to_string()))?;
        toml::from_str(&raw).map_err(|e| LxmfError::Decode(e.to_string()))
    }

    pub fn example_toml() -> String {
        let cfg = Self::default();
        toml::to_string_pretty(&cfg).expect("valid lxmd config template")
    }
}
