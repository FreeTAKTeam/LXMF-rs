use crate::profiles::required_capabilities;
use crate::types::{AuthMode, BindMode, OverflowPolicy, Profile, RpcBackendConfig};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CapabilityState {
    Enabled,
    Disabled,
    Experimental,
    Deprecated,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct CapabilityDescriptor {
    pub id: String,
    pub version: u16,
    pub state: CapabilityState,
    pub since_contract: String,
    pub deprecated_after_contract: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct EffectiveLimits {
    pub max_poll_events: usize,
    pub max_event_bytes: usize,
    pub max_batch_bytes: usize,
    pub max_extension_keys: usize,
    pub idempotency_ttl_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct NegotiationRequest {
    pub supported_contract_versions: Vec<u16>,
    pub requested_capabilities: Vec<String>,
    pub profile: Profile,
    pub bind_mode: BindMode,
    pub auth_mode: AuthMode,
    pub overflow_policy: OverflowPolicy,
    pub block_timeout_ms: Option<u64>,
    pub rpc_backend: Option<RpcBackendConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct NegotiationResponse {
    pub runtime_id: String,
    pub active_contract_version: u16,
    pub effective_capabilities: Vec<String>,
    pub effective_limits: EffectiveLimits,
    pub contract_release: String,
    pub schema_namespace: String,
}

pub fn negotiate_contract_version(
    client_supported: &[u16],
    backend_supported: &[u16],
) -> Option<u16> {
    client_supported.iter().filter(|version| backend_supported.contains(version)).max().copied()
}

pub fn effective_capabilities_for_profile(profile: Profile) -> Vec<String> {
    required_capabilities(profile).iter().map(|capability| (*capability).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::negotiate_contract_version;

    #[test]
    fn negotiate_contract_version_selects_highest_overlap() {
        let selected = negotiate_contract_version(&[1, 2], &[2]);
        assert_eq!(selected, Some(2));
    }

    #[test]
    fn negotiate_contract_version_falls_back_when_future_versions_are_advertised() {
        let selected = negotiate_contract_version(&[4, 3, 2], &[2]);
        assert_eq!(selected, Some(2));
    }

    #[test]
    fn negotiate_contract_version_returns_none_without_overlap() {
        let selected = negotiate_contract_version(&[4, 3], &[2]);
        assert_eq!(selected, None);
    }
}
