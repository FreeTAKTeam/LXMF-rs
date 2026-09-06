use super::bridge_helpers::{
    log_delivery_trace, opportunistic_payload, payload_preview, send_trace_detail,
};
use super::direct_backchannel::DirectBackchannelLinks;
#[path = "bridge_announce.rs"]
mod announce;
#[path = "bridge_delivery_method.rs"]
mod delivery_method;
#[path = "bridge_delivery_scheduler.rs"]
mod delivery_scheduler;
#[path = "bridge_delivery_task.rs"]
mod delivery_task;
pub(crate) use delivery_task::emit_receipt_event;
#[path = "bridge_delivery_task_cancel.rs"]
mod delivery_task_cancel;
#[path = "bridge_delivery_task_payload.rs"]
mod delivery_task_payload;
#[path = "bridge_delivery_task_propagation.rs"]
mod delivery_task_propagation;
#[cfg(test)]
#[path = "bridge_delivery_task_tests.rs"]
mod delivery_task_tests;
#[path = "bridge_identity.rs"]
mod identity_resolver;
#[path = "bridge_link_send.rs"]
mod link_send;
#[path = "bridge_outbound.rs"]
mod outbound;
#[path = "bridge_paper.rs"]
mod paper;
#[path = "bridge_payload.rs"]
mod payload_builder;
#[path = "bridge_propagation.rs"]
mod propagation;
#[path = "bridge_remote_control.rs"]
mod remote_control;
#[path = "bridge_remote_control_download.rs"]
mod remote_control_download;
#[path = "bridge_remote_control_link.rs"]
mod remote_control_link;
#[path = "bridge_remote_fetch.rs"]
mod remote_fetch;
#[path = "bridge_remote_request.rs"]
mod remote_request;
#[path = "bridge_service_identity.rs"]
mod service_identity;
use super::outbound_resources::{
    track_outbound_resource, OutboundResourceMap, OutboundResourceTracking,
    OUTBOUND_RESOURCE_SENT_STATUS,
};
use reticulum_daemon::receipt_bridge::{track_receipt_mapping, ReceiptEvent};
use rns_core::identity::PrivateIdentity;
use rns_rpc::RpcDaemon;
#[cfg(test)]
use rns_rpc::RpcRequest;
use rns_transport::delivery::await_link_activation;
use rns_transport::delivery::{
    send_on_link_observed, send_outcome_is_sent, send_outcome_status, LinkSendResult,
};
use rns_transport::destination::{
    link::{Link, LinkStatus},
    DestinationDesc, DestinationName, SingleInputDestination, SingleOutputDestination,
};
use rns_transport::destination_hash::parse_destination_hash_required;
use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::identity::Identity;
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use rns_transport::transport::Transport;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) use delivery_method::{validate_delivery_request, RequestedDeliveryMethod};
use delivery_scheduler::{DeliveryScheduler, DeliverySchedulerConfig};
use delivery_task::{DeliveryTask, LinkModeStatuses};
use identity_resolver::resolve_destination_identity_blocking;
#[cfg(test)]
pub(crate) use propagation::wait_for_propagation_signal;
use propagation::CachedPropagationLink;
#[cfg(test)]
pub(crate) use propagation::DEFAULT_PROPAGATION_STAMP_COST;

pub(super) struct TransportBridge {
    daemon: Arc<Mutex<Option<Arc<RpcDaemon>>>>,
    transport: Arc<Transport>,
    signer: PrivateIdentity,
    delivery_source_hash: [u8; 16],
    announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    announce_app_data: Option<Vec<u8>>,
    announce_capabilities: Vec<String>,
    propagation_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    propagation_announce_app_data: Option<Vec<u8>>,
    control_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    outbound_propagation_identities: Arc<Mutex<HashMap<String, Identity>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: OutboundResourceMap,
    outbound_propagation_link: Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    direct_backchannel_links: DirectBackchannelLinks,
    receipt_tx: tokio::sync::mpsc::Sender<ReceiptEvent>,
    delivery_scheduler: DeliveryScheduler,
    service_identities: Arc<RwLock<service_identity::ServiceIdentityRegistry>>,
    service_identity_dir: PathBuf,
    runtime_handle: tokio::runtime::Handle,
}

#[derive(Clone, Copy)]
pub(super) struct PeerCrypto {
    pub(super) identity: Identity,
}

impl TransportBridge {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        transport: Arc<Transport>,
        service_identity_dir: PathBuf,
        signer: PrivateIdentity,
        delivery_source_hash: [u8; 16],
        announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
        announce_app_data: Option<Vec<u8>>,
        announce_capabilities: Vec<String>,
        propagation_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
        propagation_announce_app_data: Option<Vec<u8>>,
        control_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
        peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
        receipt_map: Arc<Mutex<HashMap<String, String>>>,
        outbound_resource_map: OutboundResourceMap,
        receipt_tx: tokio::sync::mpsc::Sender<ReceiptEvent>,
    ) -> Self {
        let default_display_name = announce_app_data.as_deref().and_then(|app_data| {
            reticulum_daemon::announce_names::parse_peer_name_from_app_data(app_data)
                .ok()
                .flatten()
                .map(|(display_name, _)| display_name)
        });
        let bridge = Self {
            daemon: Arc::new(Mutex::new(None)),
            transport,
            signer,
            delivery_source_hash,
            announce_destination,
            announce_app_data,
            announce_capabilities,
            propagation_announce_destination,
            propagation_announce_app_data,
            control_announce_destination,
            peer_crypto,
            outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
            receipt_map,
            outbound_resource_map,
            outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
            direct_backchannel_links: DirectBackchannelLinks::new(),
            receipt_tx,
            delivery_scheduler: DeliveryScheduler::spawn(DeliverySchedulerConfig::from_env()),
            service_identities: Arc::new(RwLock::new(
                service_identity::ServiceIdentityRegistry::default(),
            )),
            service_identity_dir,
            runtime_handle: tokio::runtime::Handle::current(),
        };
        bridge.initialize_default_service_identity(default_display_name);
        bridge
    }

    pub(super) fn direct_backchannel_links(&self) -> DirectBackchannelLinks {
        self.direct_backchannel_links.clone()
    }

    pub(super) fn set_daemon(&self, daemon: Arc<RpcDaemon>) {
        match self.daemon.lock() {
            Ok(mut guard) => *guard = Some(daemon),
            Err(error) => {
                log::error!("[daemon] remote-control bridge daemon lock poisoned: {error}");
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn propagation_link_for_test(
        &self,
        node_hex: &str,
        destination: DestinationDesc,
    ) -> Arc<tokio::sync::Mutex<Link>> {
        propagation::propagation_link_for_node(
            self.transport.as_ref(),
            &self.outbound_propagation_link,
            node_hex,
            destination,
        )
        .await
    }
}

fn now_secs_i64() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(i64::MAX)
}
