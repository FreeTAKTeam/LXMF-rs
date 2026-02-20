use super::config::SdkConfig;
use crate::capability::EffectiveLimits;
use crate::error::{code, ErrorCategory, SdkError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct StartRequest {
    pub supported_contract_versions: Vec<u16>,
    pub requested_capabilities: Vec<String>,
    pub config: SdkConfig,
}

pub const DEFAULT_SUPPORTED_CONTRACT_VERSION: u16 = 2;

impl StartRequest {
    pub fn new(config: SdkConfig) -> Self {
        Self {
            supported_contract_versions: vec![DEFAULT_SUPPORTED_CONTRACT_VERSION],
            requested_capabilities: Vec::new(),
            config,
        }
    }

    pub fn with_supported_contract_versions(
        mut self,
        versions: impl IntoIterator<Item = u16>,
    ) -> Self {
        self.supported_contract_versions = versions.into_iter().collect();
        self
    }

    pub fn with_requested_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.requested_capabilities =
            capabilities.into_iter().map(Into::into).collect::<Vec<String>>();
        self
    }

    pub fn with_requested_capability(mut self, capability: impl Into<String>) -> Self {
        self.requested_capabilities.push(capability.into());
        self
    }

    pub fn validate(&self) -> Result<(), SdkError> {
        if self.supported_contract_versions.is_empty() {
            return Err(SdkError::new(
                code::CAPABILITY_CONTRACT_INCOMPATIBLE,
                ErrorCategory::Capability,
                "supported_contract_versions must not be empty",
            )
            .with_user_actionable(true));
        }

        let mut seen_versions = BTreeSet::new();
        for version in &self.supported_contract_versions {
            if !seen_versions.insert(*version) {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "supported_contract_versions must be unique",
                )
                .with_user_actionable(true));
            }
        }

        let mut seen_caps = BTreeSet::new();
        for capability in &self.requested_capabilities {
            let trimmed = capability.trim();
            if trimmed.is_empty() {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "requested capability IDs must not be empty",
                )
                .with_user_actionable(true));
            }
            if !seen_caps.insert(trimmed.to_owned()) {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "requested capability IDs must be unique",
                )
                .with_user_actionable(true));
            }
        }

        self.config.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ClientHandle {
    pub runtime_id: String,
    pub active_contract_version: u16,
    pub effective_capabilities: Vec<String>,
    pub effective_limits: EffectiveLimits,
}
