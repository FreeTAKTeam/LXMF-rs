use crate::profiles::required_capabilities;
use crate::types::{AuthMode, BindMode, OverflowPolicy, Profile, RpcBackendConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PluginState {
    Experimental,
    Stable,
    Deprecated,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginDescriptor {
    pub plugin_id: String,
    pub version: u16,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub optional_capabilities: Vec<String>,
    pub state: PluginState,
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

pub fn negotiate_plugins(
    requested_plugin_ids: &[String],
    available_plugins: &[PluginDescriptor],
    effective_capabilities: &[String],
) -> Vec<String> {
    let capabilities = effective_capabilities
        .iter()
        .map(|capability| capability.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();

    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();
    for requested in requested_plugin_ids {
        let normalized_id = requested.trim().to_ascii_lowercase();
        if normalized_id.is_empty() || !seen.insert(normalized_id.clone()) {
            continue;
        }
        let Some(descriptor) = available_plugins
            .iter()
            .find(|plugin| plugin.plugin_id.trim().eq_ignore_ascii_case(normalized_id.as_str()))
        else {
            continue;
        };

        let supported = descriptor.required_capabilities.iter().all(|required_capability| {
            capabilities.contains(required_capability.trim().to_ascii_lowercase().as_str())
        });
        if supported {
            resolved.push(descriptor.plugin_id.clone());
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::{negotiate_contract_version, negotiate_plugins, PluginDescriptor, PluginState};

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

    #[test]
    fn plugin_negotiation_selects_plugins_with_supported_required_capabilities() {
        let available = vec![
            PluginDescriptor {
                plugin_id: "domain.topics".to_owned(),
                version: 1,
                required_capabilities: vec!["sdk.capability.topics".to_owned()],
                optional_capabilities: vec!["sdk.capability.topic_fanout".to_owned()],
                state: PluginState::Stable,
            },
            PluginDescriptor {
                plugin_id: "domain.voice".to_owned(),
                version: 1,
                required_capabilities: vec![
                    "sdk.capability.voice_signaling".to_owned(),
                    "sdk.capability.mtls_auth".to_owned(),
                ],
                optional_capabilities: vec![],
                state: PluginState::Experimental,
            },
        ];

        let resolved = negotiate_plugins(
            &["domain.topics".to_owned(), "domain.voice".to_owned()],
            &available,
            &["sdk.capability.topics".to_owned(), "sdk.capability.voice_signaling".to_owned()],
        );
        assert_eq!(resolved, vec!["domain.topics".to_owned()]);
    }

    #[test]
    fn plugin_negotiation_ignores_unknown_and_duplicate_plugin_requests() {
        let available = vec![PluginDescriptor {
            plugin_id: "domain.topics".to_owned(),
            version: 1,
            required_capabilities: vec!["sdk.capability.topics".to_owned()],
            optional_capabilities: vec![],
            state: PluginState::Stable,
        }];

        let resolved = negotiate_plugins(
            &["domain.unknown".to_owned(), "domain.topics".to_owned(), "DOMAIN.TOPICS".to_owned()],
            &available,
            &["sdk.capability.topics".to_owned()],
        );
        assert_eq!(resolved, vec!["domain.topics".to_owned()]);
    }
}
