//! Router boundary crate for enterprise split.

#[derive(Clone, Debug, Default)]
pub struct RouterConfig {
    pub propagation_per_transfer_limit: u32,
    pub propagation_per_sync_limit: u32,
    pub propagation_stamp_cost: u32,
    pub propagation_stamp_cost_flexibility: u32,
    pub peering_cost: u32,
    pub auth_required: bool,
    pub transfer_state_ttl_secs: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Router {
    config: RouterConfig,
    has_adapter: bool,
}

impl Router {
    pub fn with_adapter(_adapter: impl core::fmt::Debug) -> Self {
        Self { has_adapter: true, ..Self::default() }
    }

    pub fn with_transport_plugin(_adapter: impl core::fmt::Debug) -> Self {
        Self { has_adapter: true, ..Self::default() }
    }

    pub fn has_adapter(&self) -> bool {
        self.has_adapter
    }

    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: RouterConfig) {
        self.config = config;
    }
}

pub mod router {
    pub type Router = super::Router;
    pub type RouterConfig = super::RouterConfig;
}
