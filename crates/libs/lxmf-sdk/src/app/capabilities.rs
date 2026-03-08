use crate::{ClientHandle, EffectiveLimits};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct CapabilitySummary {
    pub active_contract_version: u16,
    pub effective_capabilities: Vec<String>,
    pub effective_limits: EffectiveLimits,
}

impl From<&ClientHandle> for CapabilitySummary {
    fn from(handle: &ClientHandle) -> Self {
        Self {
            active_contract_version: handle.active_contract_version,
            effective_capabilities: handle.effective_capabilities.clone(),
            effective_limits: handle.effective_limits.clone(),
        }
    }
}
