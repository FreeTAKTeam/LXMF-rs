use super::delivery::DeliveryPlan;
use super::node::{Config, Profile};
use crate::SdkConfig;

impl Config {
    pub fn from_profile(profile: Profile) -> Self {
        match profile {
            Profile::MobileDefault => Self {
                profile,
                sdk_config: SdkConfig::desktop_local_default(),
                supported_contract_versions: vec![2],
                requested_capabilities: Vec::new(),
                event_batch_size: Some(32),
            },
            Profile::DesktopDefault => Self {
                profile,
                sdk_config: SdkConfig::desktop_full_default(),
                supported_contract_versions: vec![2],
                requested_capabilities: Vec::new(),
                event_batch_size: Some(64),
            },
            Profile::EmbeddedDefault => Self {
                profile,
                sdk_config: SdkConfig::embedded_alloc_default(),
                supported_contract_versions: vec![2],
                requested_capabilities: Vec::new(),
                event_batch_size: Some(16),
            },
            Profile::TestingDefault => {
                let mut sdk_config = SdkConfig::desktop_local_default();
                sdk_config.event_stream.max_poll_events = 32;
                Self {
                    profile,
                    sdk_config,
                    supported_contract_versions: vec![2],
                    requested_capabilities: Vec::new(),
                    event_batch_size: Some(16),
                }
            }
        }
    }

    pub fn delivery_plan(&self) -> DeliveryPlan {
        let mut plan = self.profile.defaults();
        plan.default_event_batch_size =
            self.event_batch_size.unwrap_or(plan.default_event_batch_size);
        plan.redaction_enabled = self.sdk_config.redaction.enabled;
        plan
    }

    pub fn with_event_batch_size(mut self, event_batch_size: usize) -> Self {
        self.event_batch_size = Some(event_batch_size.max(1));
        self
    }

    pub fn with_supported_contract_version(mut self, contract_version: u16) -> Self {
        self.supported_contract_versions = vec![contract_version];
        self
    }

    pub fn with_supported_contract_versions(
        mut self,
        contract_versions: impl IntoIterator<Item = u16>,
    ) -> Self {
        self.supported_contract_versions = contract_versions.into_iter().collect();
        self
    }
}
