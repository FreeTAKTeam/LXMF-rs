use super::bridge_helpers::{
    diagnostics_enabled, log_delivery_trace, opportunistic_payload, payload_preview,
    send_trace_detail,
};
#[path = "bridge_announce.rs"]
mod announce;
#[path = "bridge_delivery_method.rs"]
mod delivery_method;
#[path = "bridge_delivery_task.rs"]
mod delivery_task;
#[path = "bridge_identity.rs"]
mod identity_resolver;
#[path = "bridge_link_send.rs"]
mod link_send;
#[path = "bridge_outbound.rs"]
mod outbound;
#[path = "bridge_paper.rs"]
mod paper;
#[path = "bridge_propagation.rs"]
mod propagation;
#[path = "bridge_remote_control.rs"]
mod remote_control;
use super::inbound_worker::{
    track_outbound_resource, OutboundResourceTracking, OUTBOUND_RESOURCE_SENT_STATUS,
};
use reticulum_daemon::receipt_bridge::{track_receipt_mapping, ReceiptEvent};
use rns_core::identity::PrivateIdentity;
use rns_rpc::{RpcDaemon, RpcRequest};
use rns_transport::delivery::await_link_activation;
use rns_transport::delivery::{
    send_on_link, send_outcome_is_sent, send_outcome_status, send_via_link, LinkSendResult,
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) use delivery_method::{validate_delivery_request, RequestedDeliveryMethod};
use delivery_task::{DeliveryTask, LinkModeStatuses};
use identity_resolver::resolve_destination_identity_blocking;
use propagation::CachedPropagationLink;

pub(super) struct TransportBridge {
    daemon: Arc<Mutex<Option<Arc<RpcDaemon>>>>,
    transport: Arc<Transport>,
    signer: PrivateIdentity,
    delivery_source_hash: [u8; 16],
    announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    announce_app_data: Option<Vec<u8>>,
    propagation_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    propagation_announce_app_data: Option<Vec<u8>>,
    control_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    outbound_propagation_identities: Arc<Mutex<HashMap<String, Identity>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    outbound_propagation_link: Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
}

#[derive(Clone, Copy)]
pub(super) struct PeerCrypto {
    pub(super) identity: Identity,
}

impl TransportBridge {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        transport: Arc<Transport>,
        signer: PrivateIdentity,
        delivery_source_hash: [u8; 16],
        announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
        announce_app_data: Option<Vec<u8>>,
        propagation_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
        propagation_announce_app_data: Option<Vec<u8>>,
        control_announce_destination: Option<Arc<tokio::sync::Mutex<SingleInputDestination>>>,
        peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
        receipt_map: Arc<Mutex<HashMap<String, String>>>,
        outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
        receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self {
            daemon: Arc::new(Mutex::new(None)),
            transport,
            signer,
            delivery_source_hash,
            announce_destination,
            announce_app_data,
            propagation_announce_destination,
            propagation_announce_app_data,
            control_announce_destination,
            peer_crypto,
            outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
            receipt_map,
            outbound_resource_map,
            outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
            receipt_tx,
        }
    }

    pub(super) fn set_daemon(&self, daemon: Arc<RpcDaemon>) {
        if let Ok(mut guard) = self.daemon.lock() {
            *guard = Some(daemon);
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
