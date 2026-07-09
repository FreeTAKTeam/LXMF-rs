use std::sync::Arc;
use std::time::Duration;

use rns_transport::hash::AddressHash;
use rns_transport::identity::PrivateIdentity;
use rns_transport::transport::Transport;
use tokio::runtime::Handle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InProcessBackendLimits {
    pub event_retention: usize,
    pub delivery_retention: usize,
    pub send_report_retention: usize,
}

impl Default for InProcessBackendLimits {
    fn default() -> Self {
        Self { event_retention: 2_048, delivery_retention: 1_024, send_report_retention: 512 }
    }
}

#[derive(Clone)]
pub struct InProcessBackendConfig {
    pub runtime_id: String,
    pub runtime_handle: Handle,
    pub transport: Arc<Transport>,
    pub identity: PrivateIdentity,
    pub source_destination: AddressHash,
    pub propagation_relay: Option<AddressHash>,
    pub link_connect_timeout: Duration,
    pub link_connect_attempts: usize,
    pub resource_transfer_timeout: Duration,
    pub limits: InProcessBackendLimits,
}

impl InProcessBackendConfig {
    pub fn new(
        runtime_id: impl Into<String>,
        runtime_handle: Handle,
        transport: Arc<Transport>,
        identity: PrivateIdentity,
        source_destination: AddressHash,
    ) -> Self {
        Self {
            runtime_id: runtime_id.into(),
            runtime_handle,
            transport,
            identity,
            source_destination,
            propagation_relay: None,
            link_connect_timeout: Duration::from_secs(20),
            link_connect_attempts: 3,
            resource_transfer_timeout: Duration::from_secs(120),
            limits: InProcessBackendLimits::default(),
        }
    }
}
