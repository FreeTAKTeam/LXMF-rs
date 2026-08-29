use super::*;
use crate::resource::{DEFAULT_RESOURCE_MAX_RETRIES, DEFAULT_RESOURCE_RETRY_INTERVAL_SECS};

impl TransportConfig {
    pub fn new<T: Into<String>>(name: T, identity: &PrivateIdentity, broadcast: bool) -> Self {
        Self {
            name: name.into(),
            identity: identity.clone(),
            broadcast,
            transport_enabled: false,
            connected_to_shared_instance: false,
            local_hops_delta: 0,
            announce_cache_capacity: 100_000,
            announce_retry_limit: 1,
            announce_queue_len: 64,
            announce_cap: 128,
            path_request_timeout_secs: 30,
            link_proof_timeout_secs: 600,
            link_idle_timeout_secs: 900,
            inbound_queue_limits: InboundQueueLimits::default(),
            resource_retry_interval_secs: DEFAULT_RESOURCE_RETRY_INTERVAL_SECS,
            resource_retry_limit: DEFAULT_RESOURCE_MAX_RETRIES,
            ratchet_store_path: None,
        }
    }

    /// Enables or disables forwarding traffic for remote destinations and links.
    ///
    /// This is the in-process equivalent of Reticulum's
    /// `[reticulum] enable_transport` setting. Disabled transports still serve
    /// their own destinations and locally owned links.
    pub fn set_transport_enabled(&mut self, enabled: bool) {
        self.transport_enabled = enabled;
    }

    /// Compatibility alias for callers that used the original transport-mode setter.
    pub fn set_retransmit(&mut self, retransmit: bool) {
        self.set_transport_enabled(retransmit);
    }

    pub fn set_connected_to_shared_instance(&mut self, connected: bool) {
        self.connected_to_shared_instance = connected;
    }

    pub fn set_local_hops_delta(&mut self, delta: u8) {
        self.local_hops_delta = delta;
    }

    pub fn set_broadcast(&mut self, broadcast: bool) {
        self.broadcast = broadcast;
    }

    pub fn set_announce_cache_capacity(&mut self, capacity: usize) {
        self.announce_cache_capacity = capacity;
    }

    pub fn set_announce_retry_limit(&mut self, limit: u8) {
        self.announce_retry_limit = limit;
    }

    pub fn set_announce_queue_len(&mut self, len: usize) {
        self.announce_queue_len = len;
    }

    pub fn set_announce_cap(&mut self, cap: usize) {
        self.announce_cap = cap;
    }

    pub fn set_path_request_timeout_secs(&mut self, secs: u64) {
        self.path_request_timeout_secs = secs;
    }

    pub fn set_link_proof_timeout_secs(&mut self, secs: u64) {
        self.link_proof_timeout_secs = secs;
    }

    /// Sets propagated LinkTable retention only; direct-link keepalive/stale timing remains per-link RTT-driven.
    pub fn set_link_idle_timeout_secs(&mut self, secs: u64) {
        self.link_idle_timeout_secs = secs;
    }

    pub fn set_inbound_queue_limits(
        &mut self,
        limits: InboundQueueLimits,
    ) -> Result<(), &'static str> {
        if !limits.is_valid() {
            return Err("inbound queue limits must all be non-zero");
        }
        self.inbound_queue_limits = limits;
        Ok(())
    }

    pub fn set_resource_retry_interval_secs(&mut self, secs: u64) {
        self.resource_retry_interval_secs = secs;
    }

    pub fn set_resource_retry_limit(&mut self, limit: u8) {
        self.resource_retry_limit = limit;
    }

    pub fn set_ratchet_store_path(&mut self, path: PathBuf) {
        self.ratchet_store_path = Some(path);
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            name: "tp".into(),
            identity: PrivateIdentity::new_from_rand(OsRng),
            broadcast: false,
            transport_enabled: false,
            connected_to_shared_instance: false,
            local_hops_delta: 0,
            announce_cache_capacity: 100_000,
            announce_retry_limit: 1,
            announce_queue_len: 64,
            announce_cap: 128,
            path_request_timeout_secs: 30,
            link_proof_timeout_secs: 600,
            link_idle_timeout_secs: 900,
            inbound_queue_limits: InboundQueueLimits::default(),
            resource_retry_interval_secs: DEFAULT_RESOURCE_RETRY_INTERVAL_SECS,
            resource_retry_limit: DEFAULT_RESOURCE_MAX_RETRIES,
            ratchet_store_path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resource_retry_budget_matches_reference_implementation() {
        let config = TransportConfig::default();

        assert_eq!(config.resource_retry_interval_secs, 2);
        assert_eq!(config.resource_retry_limit, 16);
    }

    #[test]
    fn retransmit_setter_remains_a_transport_enabled_alias() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let mut config = TransportConfig::new("test", &identity, true);

        config.set_retransmit(true);
        assert!(config.transport_enabled);
        config.set_transport_enabled(false);
        assert!(!config.transport_enabled);
    }

    #[test]
    fn rns_1_5_queue_limits_default_and_validate_like_python() {
        let mut config = TransportConfig::default();
        assert_eq!(config.inbound_queue_limits, InboundQueueLimits::default());
        assert!(config
            .set_inbound_queue_limits(InboundQueueLimits {
                data: 0,
                ..InboundQueueLimits::default()
            })
            .is_err());
    }

    #[tokio::test]
    async fn rns_1_5_runtime_accessors_report_empty_runtime_consistently() {
        let transport = Transport::new(TransportConfig::default());
        assert_eq!(Transport::default_data_queue_length(), 1024);
        assert_eq!(Transport::default_announce_queue_length(), 128);
        assert_eq!(Transport::default_path_request_queue_length(), 128);
        assert_eq!(Transport::default_ingress_limited_queue_length(), 8);
        assert_eq!(transport.link_count().await, 0);
        assert_eq!(transport.active_link_count().await, 0);
        assert_eq!(transport.lowest_interface_bitrate().await, None);
        assert_eq!(transport.medium_path_timeout().await, Duration::ZERO);
        assert_eq!(
            transport.inbound_queue_snapshot().await,
            InboundQueueSnapshot {
                limits: InboundQueueLimits::default().as_array(),
                ..InboundQueueSnapshot::default()
            }
        );
    }
}
