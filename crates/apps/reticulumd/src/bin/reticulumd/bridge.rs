use super::bridge_helpers::{
    diagnostics_enabled, log_delivery_trace, opportunistic_payload, payload_preview,
    send_trace_detail,
};
use super::inbound_worker::{
    track_outbound_resource, OutboundResourceTracking, OUTBOUND_RESOURCE_SENT_STATUS,
};
use lxmf::WireMessage;
use rand_core::OsRng;
use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
use reticulum_daemon::lxmf_bridge::rmpv_to_json;
use reticulum_daemon::lxmf_stamps::generate_propagation_stamp;
use reticulum_daemon::receipt_bridge::{track_receipt_mapping, ReceiptEvent};
use rns_core::identity::{Identity as CoreIdentity, PrivateIdentity};
use rns_rpc::{
    AnnounceBridge, OutboundBridge, OutboundDeliveryOptions, RemoteControlBridge, RpcDaemon,
    RpcRequest,
};
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

const PROPAGATION_INVALID_STAMP_SIGNAL: u8 = 0xF5;
const DEFAULT_PROPAGATION_STAMP_COST: u32 = 13;

#[derive(Clone)]
struct CachedPropagationLink {
    node_hex: String,
    link: Arc<tokio::sync::Mutex<Link>>,
}

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
        propagation_link_for_node(
            self.transport.as_ref(),
            &self.outbound_propagation_link,
            node_hex,
            destination,
        )
        .await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestedDeliveryMethod {
    Opportunistic,
    Direct,
    Propagated,
    Paper,
}

impl RequestedDeliveryMethod {
    pub(crate) fn parse(method: Option<&str>) -> Result<Self, std::io::Error> {
        let normalized = method.map(str::trim).unwrap_or_default().to_ascii_lowercase();
        match normalized.as_str() {
            "" | "direct" => Ok(Self::Direct),
            "opportunistic" => Ok(Self::Opportunistic),
            "propagated" => Ok(Self::Propagated),
            "paper" => Ok(Self::Paper),
            other => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported delivery method '{other}'"),
            )),
        }
    }
}

pub(crate) fn validate_delivery_request(
    method: RequestedDeliveryMethod,
    propagation_node: Option<&str>,
) -> Result<(), std::io::Error> {
    match method {
        RequestedDeliveryMethod::Propagated => {
            if propagation_node.is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no outbound propagation node selected",
                ));
            }
            Ok(())
        }
        RequestedDeliveryMethod::Paper
        | RequestedDeliveryMethod::Opportunistic
        | RequestedDeliveryMethod::Direct => Ok(()),
    }
}

struct LinkModeStatuses {
    packet: &'static str,
    resource: &'static str,
    resource_sent: &'static str,
}

struct DeliveryTask {
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    outbound_propagation_identities: Arc<Mutex<HashMap<String, Identity>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    outbound_propagation_link: Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    message_id: String,
    destination: [u8; 16],
    destination_hash: AddressHash,
    destination_hex: String,
    payload: Vec<u8>,
    peer_identity: Option<Identity>,
    propagation_node_identity: Option<Identity>,
    requested_method: RequestedDeliveryMethod,
    try_propagation_on_fail: bool,
    propagation_node_hex: Option<String>,
}

impl DeliveryTask {
    fn cached_identity_candidates(&self) -> Vec<Identity> {
        let mut candidates = Vec::new();

        let mut push_candidate = |identity: Identity| {
            let already_present = candidates.iter().any(|existing: &Identity| {
                existing.public_key_bytes() == identity.public_key_bytes()
                    && existing.verifying_key_bytes() == identity.verifying_key_bytes()
            });
            if !already_present {
                candidates.push(identity);
            }
        };

        if let Some(identity) = self.peer_identity {
            push_candidate(identity);
        }
        if let Some(identity) = self.propagation_node_identity {
            push_candidate(identity);
        }
        if let Ok(peers) = self.peer_crypto.lock() {
            for info in peers.values() {
                push_candidate(info.identity);
            }
        }
        if let Ok(identities) = self.outbound_propagation_identities.lock() {
            for identity in identities.values() {
                push_candidate(*identity);
            }
        }

        candidates
    }

    fn cached_identity_for_destination(&self, destination_hash: AddressHash) -> Option<Identity> {
        const LXMF_ASPECTS: [&str; 3] = ["delivery", "propagation", "propagation.control"];

        self.cached_identity_candidates().into_iter().find(|identity| {
            LXMF_ASPECTS.iter().any(|aspect| {
                SingleOutputDestination::new(*identity, DestinationName::new("lxmf", aspect))
                    .desc
                    .address_hash
                    == destination_hash
            })
        })
    }

    async fn run(self) {
        log_delivery_trace(&self.message_id, &self.destination_hex, "start", "delivery requested");
        match self.requested_method {
            RequestedDeliveryMethod::Direct => self.run_direct().await,
            RequestedDeliveryMethod::Opportunistic => self.run_opportunistic().await,
            RequestedDeliveryMethod::Propagated => self.run_propagated().await,
            RequestedDeliveryMethod::Paper => {}
        }
    }

    async fn run_direct(self) {
        let Some(identity) = self.resolve_destination_identity().await else {
            return;
        };
        let destination_desc = DestinationDesc {
            identity,
            address_hash: self.destination_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };

        match self
            .send_via_link_mode(
                "link",
                self.destination_hex.as_str(),
                destination_desc,
                &self.payload,
                LinkModeStatuses {
                    packet: "sent: link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        {
            Ok(()) => {}
            Err(err) if self.try_propagation_on_fail && self.propagation_node_hex.is_some() => {
                let detail = format!("direct failed err={err}; trying propagated");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: format!("link failed: {err}; trying propagated"),
                });
                self.run_propagated().await;
            }
            Err(err) => {
                let detail = format!("direct failed err={err}");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
            }
        }
    }

    async fn run_propagated(self) {
        let Some(destination_identity) = self.resolve_destination_identity().await else {
            return;
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "recipient identity ready",
        );
        let Some(propagation_node_hex) = self.propagation_node_hex.clone() else {
            let _ = self.receipt_tx.send(ReceiptEvent {
                message_id: self.message_id,
                status: "failed: no outbound propagation node selected".to_string(),
            });
            return;
        };

        let propagation_hash = match parse_destination_hash_required(&propagation_node_hex) {
            Ok(hash) => AddressHash::new(hash),
            Err(err) => {
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "selected propagation node parsed",
        );
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "looking up propagation stamp cost",
        );
        let target_cost = self
            .propagation_target_cost(propagation_node_hex.as_str())
            .unwrap_or(DEFAULT_PROPAGATION_STAMP_COST);
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!("using propagation stamp cost={target_cost}").as_str(),
        );
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "building propagation payload",
        );
        let payload =
            match build_propagation_payload(&self.payload, &destination_identity, target_cost) {
                Ok(payload) => payload,
                Err(err) => {
                    let _ = self.receipt_tx.send(ReceiptEvent {
                        message_id: self.message_id,
                        status: format!("failed: {err}"),
                    });
                    return;
                }
            };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!("propagation payload ready bytes={}", payload.len()).as_str(),
        );

        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "resolving propagation link",
        );
        let propagation_link = match self
            .resolve_or_create_propagation_link(&propagation_node_hex, propagation_hash)
            .await
        {
            Ok(link) => link,
            Err(err) => {
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "propagation link ready",
        );

        if let Err(err) = self
            .send_via_existing_link_mode(
                "propagation",
                propagation_node_hex.as_str(),
                propagation_link,
                &payload,
                LinkModeStatuses {
                    packet: "sent: propagated",
                    resource: "sending: propagated resource",
                    resource_sent: "sent: propagated resource",
                },
            )
            .await
        {
            let detail = format!("propagated failed err={err}");
            log_delivery_trace(&self.message_id, &self.destination_hex, "propagation", &detail);
            let _ = self.receipt_tx.send(ReceiptEvent {
                message_id: self.message_id,
                status: format!("failed: {err}"),
            });
        }
    }

    async fn run_opportunistic(self) {
        // Opportunistic SINGLE packets must carry LXMF wire bytes
        // without the destination prefix. Receivers prepend the
        // packet destination hash before unpacking.
        let opportunistic_payload = opportunistic_payload(&self.payload, &self.destination);
        let mut data = PacketDataBuffer::new();
        if data.write(opportunistic_payload).is_err() {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "payload too large",
            );
            let _ = self.receipt_tx.send(ReceiptEvent {
                message_id: self.message_id,
                status: "failed: opportunistic payload too large".to_string(),
            });
            return;
        }

        let packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: self.destination_hash,
            transport: None,
            context: PacketContext::None,
            data,
        };
        let packet_hash = hex::encode(packet.hash().to_bytes());
        track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
        if diagnostics_enabled() {
            let detail = format!(
                "sending packet_hash={} payload_len={} payload_prefix={}",
                packet_hash,
                opportunistic_payload.len(),
                payload_preview(opportunistic_payload, 16)
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &detail);
        } else {
            log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", "sending");
        }
        let trace = self.transport.send_packet_with_trace(packet).await;
        let trace_detail = send_trace_detail(trace);
        log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &trace_detail);
        let outcome = trace.outcome;
        if !send_outcome_is_sent(outcome) {
            if let Ok(mut map) = self.receipt_map.lock() {
                map.remove(&packet_hash);
            }
        }
        let _ = self.receipt_tx.send(ReceiptEvent {
            message_id: self.message_id,
            status: send_outcome_status("opportunistic", outcome),
        });
    }

    async fn resolve_destination_identity(&self) -> Option<Identity> {
        let identity = self
            .resolve_identity(
                Some(self.destination_hex.as_str()),
                self.destination_hash,
                self.peer_identity,
                "identity",
                "failed: peer not announced",
            )
            .await?;

        if let Ok(mut peers) = self.peer_crypto.lock() {
            peers.insert(self.destination_hex.clone(), PeerCrypto { identity });
        }
        Some(identity)
    }

    fn propagation_target_cost(&self, propagation_node_hex: &str) -> Option<u32> {
        let response = self
            .daemon
            .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
            .ok()?
            .result?;
        response
            .get("peers")
            .and_then(|value| value.as_array())
            .and_then(|rows| {
                rows.iter().find(|row| {
                    row.get("peer").and_then(|value| value.as_str()) == Some(propagation_node_hex)
                })
            })
            .and_then(|row| row.get("propagation_stamp_cost"))
            .and_then(|value| value.as_u64())
            .and_then(|value| u32::try_from(value).ok())
    }

    async fn resolve_or_create_propagation_link(
        &self,
        propagation_node_hex: &str,
        propagation_hash: AddressHash,
    ) -> Result<Arc<tokio::sync::Mutex<Link>>, std::io::Error> {
        if let Some(link) =
            cached_propagation_link(&self.outbound_propagation_link, propagation_node_hex).await
        {
            return Ok(link);
        }

        let cached_identity = self
            .propagation_node_identity
            .or_else(|| {
                self.outbound_propagation_identities
                    .lock()
                    .ok()
                    .and_then(|guard| guard.get(propagation_node_hex).cloned())
            })
            .or_else(|| {
                resolve_destination_identity_blocking(
                    self.transport.clone(),
                    propagation_hash,
                    Duration::from_secs(12),
                )
            });
        let Some(propagation_identity) = self
            .resolve_identity(
                Some(propagation_node_hex),
                propagation_hash,
                cached_identity,
                "propagation-node",
                "failed: propagation node not announced",
            )
            .await
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "propagation node not announced",
            ));
        };
        if let Ok(mut guard) = self.outbound_propagation_identities.lock() {
            guard.insert(propagation_node_hex.to_string(), propagation_identity);
        }

        let propagation_destination = SingleOutputDestination::new(
            propagation_identity,
            DestinationName::new("lxmf", "propagation"),
        );

        Ok(propagation_link_for_node(
            self.transport.as_ref(),
            &self.outbound_propagation_link,
            propagation_node_hex,
            propagation_destination.desc,
        )
        .await)
    }

    async fn resolve_identity(
        &self,
        destination_hex: Option<&str>,
        destination_hash: AddressHash,
        cached: Option<Identity>,
        stage: &str,
        failure_status: &str,
    ) -> Option<Identity> {
        let mut identity = cached;
        self.transport.request_path(&destination_hash, None, None).await;
        log_delivery_trace(&self.message_id, &self.destination_hex, stage, "path-requested");

        if identity.is_none() {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "waiting for announce");
            let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
            while tokio::time::Instant::now() < deadline {
                if let Some(found) = self.transport.destination_identity(&destination_hash).await {
                    identity = Some(found);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        if identity.is_none() {
            identity = self.cached_identity_for_destination(destination_hash);
            if identity.is_some() {
                let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
                log_delivery_trace(
                    &self.message_id,
                    detail,
                    stage,
                    "resolved from cached peer identity",
                );
            }
        }

        let Some(identity) = identity else {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "not found");
            let _ = self.receipt_tx.send(ReceiptEvent {
                message_id: self.message_id.clone(),
                status: failure_status.to_string(),
            });
            return None;
        };

        let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
        log_delivery_trace(&self.message_id, detail, stage, "resolved");
        Some(identity)
    }

    async fn send_via_link_mode(
        &self,
        trace_stage: &str,
        activity_peer: &str,
        destination_desc: DestinationDesc,
        payload: &[u8],
        statuses: LinkModeStatuses,
    ) -> Result<(), std::io::Error> {
        let result = send_via_link(
            self.transport.as_ref(),
            destination_desc,
            payload,
            Duration::from_secs(20),
        )
        .await;
        if diagnostics_enabled() {
            let payload_starts_with_dst =
                payload.len() >= 16 && payload[..16] == self.destination[..];
            let detail = format!(
                "payload_len={} payload_prefix={} starts_with_dst={}",
                payload.len(),
                payload_preview(payload, 16),
                payload_starts_with_dst
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, "payload", &detail);
        }

        match result {
            Ok(LinkSendResult::Packet(packet)) => {
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), true);
                let packet_hash = hex::encode(packet.hash().to_bytes());
                track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
                let detail = if diagnostics_enabled() {
                    format!(
                        "packet_hash={} packet_data_len={} packet_data_prefix={}",
                        packet_hash,
                        packet.data.len(),
                        payload_preview(packet.data.as_slice(), 16)
                    )
                } else {
                    format!("packet_hash={packet_hash}")
                };
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.packet.to_string(),
                });
                Ok(())
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.as_slice());
                track_outbound_resource(
                    &self.outbound_resource_map,
                    resource_hash_hex.clone(),
                    OutboundResourceTracking {
                        message_id: self.message_id.clone(),
                        peer: activity_peer.to_string(),
                        bytes: payload.len(),
                        sent_status: statuses.resource_sent.to_string(),
                    },
                );
                let detail = format!("resource_hash={resource_hash_hex}");
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.resource.to_string(),
                });
                Ok(())
            }
            Err(err) => {
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), false);
                Err(err)
            }
        }
    }

    async fn send_via_existing_link_mode(
        &self,
        trace_stage: &str,
        activity_peer: &str,
        link: Arc<tokio::sync::Mutex<Link>>,
        payload: &[u8],
        statuses: LinkModeStatuses,
    ) -> Result<(), std::io::Error> {
        await_link_activation(self.transport.as_ref(), &link, Duration::from_secs(20)).await?;
        let mut propagation_signal_rx =
            (trace_stage == "propagation").then(|| self.transport.received_data_events());
        let result = send_on_link(self.transport.as_ref(), &link, payload).await;
        let destination_desc = *link.lock().await.destination();
        let link_id = *link.lock().await.id();
        match result {
            Ok(LinkSendResult::Packet(packet)) => {
                let packet_hash = hex::encode(packet.hash().to_bytes());
                track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
                let detail = format!("packet_hash={packet_hash}");
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                if let Some(ref mut signal_rx) = propagation_signal_rx {
                    if let Some(signal) =
                        wait_for_propagation_signal(signal_rx, link_id, Duration::from_millis(1500))
                            .await
                    {
                        if signal == PROPAGATION_INVALID_STAMP_SIGNAL {
                            return Err(std::io::Error::other(
                                "propagation node rejected message: invalid stamp",
                            ));
                        }
                        let detail = format!("signal=0x{signal:02x}");
                        log_delivery_trace(
                            &self.message_id,
                            &self.destination_hex,
                            "propagation",
                            &detail,
                        );
                    }
                }
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), true);
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.packet.to_string(),
                });
                Ok(())
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.to_bytes());
                track_outbound_resource(
                    &self.outbound_resource_map,
                    resource_hash_hex.clone(),
                    OutboundResourceTracking {
                        message_id: self.message_id.clone(),
                        peer: activity_peer.to_string(),
                        bytes: payload.len(),
                        sent_status: statuses.resource_sent.to_string(),
                    },
                );
                let detail = format!(
                    "resource_hash={} bytes={} peer={} destination={}",
                    resource_hash_hex,
                    payload.len(),
                    activity_peer,
                    destination_desc.address_hash
                );
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                let _ = self.receipt_tx.send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.resource.to_string(),
                });
                Ok(())
            }
            Err(err) => Err(err),
        }
    }
}

async fn cached_propagation_link(
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
) -> Option<Arc<tokio::sync::Mutex<Link>>> {
    let mut guard = state.lock().await;
    let cached = guard.clone()?;

    if cached.node_hex != node_hex {
        *guard = None;
        return None;
    }

    if cached.link.lock().await.status() == LinkStatus::Closed {
        *guard = None;
        return None;
    }

    Some(cached.link)
}

async fn propagation_link_for_node(
    transport: &Transport,
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
    destination: DestinationDesc,
) -> Arc<tokio::sync::Mutex<Link>> {
    if let Some(link) = cached_propagation_link(state, node_hex).await {
        return link;
    }

    let link = transport.link(destination).await;
    let mut guard = state.lock().await;
    *guard = Some(CachedPropagationLink { node_hex: node_hex.to_string(), link: link.clone() });
    link
}

fn resolve_destination_identity_blocking(
    transport: Arc<Transport>,
    destination_hash: AddressHash,
    timeout: Duration,
) -> Option<Identity> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        runtime.block_on(async move {
            let mut identity = transport.destination_identity(&destination_hash).await;
            if identity.is_none() {
                transport.request_path(&destination_hash, None, None).await;
                let deadline = tokio::time::Instant::now() + timeout;
                while identity.is_none() && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    identity = transport.destination_identity(&destination_hash).await;
                }
            }
            identity
        })
    })
    .join()
    .ok()
    .flatten()
}

impl OutboundBridge for TransportBridge {
    fn deliver(
        &self,
        record: &rns_rpc::MessageRecord,
        options: &OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let destination = parse_destination_hash_required(&record.destination)?;
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);
        let daemon = self
            .daemon
            .lock()
            .expect("transport bridge daemon mutex poisoned")
            .clone()
            .ok_or_else(|| std::io::Error::other("daemon bridge unavailable"))?;

        let include_ticket = if options.include_ticket {
            Some(
                daemon
                    .ensure_ticket(record.destination.as_str(), None)
                    .map_err(std::io::Error::other)?,
            )
        } else {
            None
        };
        let include_ticket_bytes = include_ticket
            .as_ref()
            .map(|ticket| {
                hex::decode(ticket.ticket.as_str())
                    .map(|bytes| (ticket.expires_at, bytes))
                    .map_err(std::io::Error::other)
            })
            .transpose()?;

        let payload = build_wire_message_with_options(
            self.delivery_source_hash,
            destination,
            &record.title,
            &record.content,
            record.fields.clone(),
            &self.signer,
            options.stamp_cost,
            options.ticket.as_deref(),
            include_ticket_bytes
                .as_ref()
                .map(|(expires_at, ticket)| (*expires_at, ticket.as_slice())),
        )
        .map_err(std::io::Error::other)?;
        let requested_method = RequestedDeliveryMethod::parse(options.method.as_deref())?;
        let propagation_node_hex = daemon.outbound_propagation_node();
        validate_delivery_request(requested_method, propagation_node_hex.as_deref())?;
        let propagation_node_identity = if requested_method == RequestedDeliveryMethod::Propagated {
            propagation_node_hex.as_deref().and_then(|node_hex| {
                self.outbound_propagation_identities
                    .lock()
                    .ok()
                    .and_then(|guard| guard.get(node_hex).cloned())
                    .or_else(|| {
                        let hash = parse_destination_hash_required(node_hex).ok()?;
                        let hash = AddressHash::new(hash);
                        let identity = resolve_destination_identity_blocking(
                            self.transport.clone(),
                            hash,
                            Duration::from_secs(12),
                        )?;
                        if let Ok(mut guard) = self.outbound_propagation_identities.lock() {
                            guard.insert(node_hex.to_string(), identity);
                        }
                        Some(identity)
                    })
            })
        } else {
            None
        };
        if requested_method == RequestedDeliveryMethod::Paper {
            log_delivery_trace(
                &record.id,
                &record.destination,
                "paper",
                "deferred to sdk_paper_encode_v2",
            );
            return Ok(());
        }

        let task = DeliveryTask {
            daemon,
            transport: self.transport.clone(),
            peer_crypto: self.peer_crypto.clone(),
            outbound_propagation_identities: self.outbound_propagation_identities.clone(),
            receipt_map: self.receipt_map.clone(),
            outbound_resource_map: self.outbound_resource_map.clone(),
            outbound_propagation_link: self.outbound_propagation_link.clone(),
            receipt_tx: self.receipt_tx.clone(),
            message_id: record.id.clone(),
            destination,
            destination_hash: AddressHash::new(destination),
            destination_hex: record.destination.clone(),
            payload,
            peer_identity,
            propagation_node_identity,
            requested_method,
            try_propagation_on_fail: options.try_propagation_on_fail,
            propagation_node_hex,
        };
        tokio::spawn(task.run());
        Ok(())
    }
}

fn build_propagation_payload(
    payload: &[u8],
    destination_identity: &Identity,
    propagation_stamp_cost: u32,
) -> Result<Vec<u8>, std::io::Error> {
    let wire = WireMessage::unpack(payload).map_err(std::io::Error::other)?;
    let core_identity = CoreIdentity::new_from_slices(
        destination_identity.public_key_bytes(),
        destination_identity.verifying_key_bytes(),
    );
    let (lxmf_data, transient_id) = wire
        .pack_propagation_transient_with_rng(&core_identity, OsRng)
        .map_err(std::io::Error::other)?;
    let propagation_stamp = generate_propagation_stamp(&transient_id, propagation_stamp_cost)
        .ok_or_else(|| std::io::Error::other("failed to generate propagation stamp"))?;
    WireMessage::pack_propagation_envelope(
        now_secs_f64(),
        &lxmf_data,
        Some(propagation_stamp.as_slice()),
    )
    .map_err(std::io::Error::other)
}

fn now_secs_f64() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

impl AnnounceBridge for TransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        let transport = self.transport.clone();
        let destination = self.announce_destination.clone();
        let app_data = self.announce_app_data.clone();
        let propagation_destination = self.propagation_announce_destination.clone();
        let propagation_app_data = self.propagation_announce_app_data.clone();
        let control_destination = self.control_announce_destination.clone();
        tokio::spawn(async move {
            transport.send_announce(&destination, app_data.as_deref()).await;
            if let Some(destination) = propagation_destination.as_ref() {
                transport.send_announce(destination, propagation_app_data.as_deref()).await;
            }
            if let Some(destination) = control_destination.as_ref() {
                transport.send_announce(destination, None).await;
            }
        });
        Ok(())
    }
}

impl RemoteControlBridge for TransportBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/get/stats",
            rmpv::Value::Nil,
        )
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/peer/sync",
            remote_peer_value(peer)?,
        )
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/peer/unpeer",
            remote_peer_value(peer)?,
        )
    }
}

impl TransportBridge {
    fn run_remote_control(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        path: &str,
        data: rmpv::Value,
    ) -> Result<JsonValue, std::io::Error> {
        let remote = remote.trim().to_string();
        let identity_override = identity_private_key_hex
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                let bytes = hex::decode(value).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("identity_private_key_hex must be hex-encoded: {err}"),
                    )
                })?;
                PrivateIdentity::from_private_key_bytes(&bytes).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("invalid identity private key: {err:?}"),
                    )
                })
            })
            .transpose()?;
        let request_identity = identity_override.unwrap_or_else(|| self.signer.clone());
        let timeout = Duration::from_secs_f64(timeout_secs.max(0.1));
        let transport = self.transport.clone();
        let identity_cache = self.outbound_propagation_identities.clone();

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let result = remote_control_request(
                    transport.as_ref(),
                    &request_identity,
                    &remote,
                    path,
                    data,
                    timeout,
                )
                .await;
                if let Ok((_, identity)) = &result {
                    if let Ok(mut guard) = identity_cache.lock() {
                        guard.insert(remote.clone(), *identity);
                    }
                }
                result.map(|(json, _)| json)
            })
        })
    }
}

fn remote_peer_value(peer: &str) -> Result<rmpv::Value, std::io::Error> {
    let peer_hash = parse_destination_hash_required(peer)?;
    Ok(rmpv::Value::Binary(peer_hash.to_vec()))
}

async fn remote_control_request(
    transport: &Transport,
    request_identity: &PrivateIdentity,
    remote: &str,
    path: &str,
    data: rmpv::Value,
    timeout: Duration,
) -> Result<(JsonValue, Identity), std::io::Error> {
    let remote_hash = AddressHash::new(parse_destination_hash_required(remote)?);
    let mut remote_identity = transport.destination_identity(&remote_hash).await;
    if remote_identity.is_none() {
        transport.request_path(&remote_hash, None, None).await;
        let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_secs(12));
        while remote_identity.is_none() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            remote_identity = transport.destination_identity(&remote_hash).await;
        }
    }
    let remote_identity = remote_identity.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no path known for propagation control node",
        )
    })?;

    let destination = SingleOutputDestination::new(
        remote_identity,
        DestinationName::new("lxmf", "propagation.control"),
    );
    let link = transport.link(destination.desc).await;
    await_link_activation(transport, &link, timeout).await?;
    let link_id = *link.lock().await.id();

    let identify_payload = build_link_identify_payload(request_identity, &link_id);
    send_link_context_packet(
        transport,
        &link,
        PacketContext::LinkIdentify,
        identify_payload.as_slice(),
    )
    .await?;

    let mut data_rx = transport.received_data_events();
    let mut resource_rx = transport.resource_events();
    let request_payload = build_link_request_payload(path, data)?;
    let request_id = send_link_context_packet(
        transport,
        &link,
        PacketContext::Request,
        request_payload.as_slice(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("missing remote control request id"))?;

    let response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        destination.desc.address_hash,
        link_id,
        request_id,
        timeout,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;

    response_to_json(&response).map(|json| (json, remote_identity))
}

fn response_to_json(response: &rmpv::Value) -> Result<JsonValue, std::io::Error> {
    if let Some(code) = response.as_u64().or_else(|| response.as_i64().map(|value| value as u64)) {
        let (kind, message) = match code as u8 {
            0xF0 => (std::io::ErrorKind::PermissionDenied, "propagation node requires identity"),
            0xF1 => (std::io::ErrorKind::PermissionDenied, "propagation node denied access"),
            0xF4 => (std::io::ErrorKind::InvalidInput, "propagation node rejected the request"),
            0xFD => (std::io::ErrorKind::NotFound, "propagation peer not found"),
            _ => (std::io::ErrorKind::InvalidData, "unexpected propagation control response"),
        };
        return Err(std::io::Error::new(kind, message));
    }
    if let Some(json) = rmpv_to_json(response) {
        return Ok(json);
    }
    match response {
        rmpv::Value::Boolean(value) => Ok(json!(value)),
        rmpv::Value::Nil => Ok(JsonValue::Null),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported propagation control response payload",
        )),
    }
}

fn build_link_identify_payload(identity: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
    let mut public_key = Vec::with_capacity(64);
    public_key.extend_from_slice(identity.as_identity().public_key.as_bytes());
    public_key.extend_from_slice(identity.as_identity().verifying_key.as_bytes());

    let mut signed_data = Vec::with_capacity(16 + public_key.len());
    signed_data.extend_from_slice(link_id.as_slice());
    signed_data.extend_from_slice(public_key.as_slice());
    let signature = identity.sign(signed_data.as_slice());

    let mut payload = Vec::with_capacity(public_key.len() + signature.to_bytes().len());
    payload.extend_from_slice(public_key.as_slice());
    payload.extend_from_slice(signature.to_bytes().as_slice());
    payload
}

async fn wait_for_propagation_signal(
    rx: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    timeout: Duration,
) -> Option<u8> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let Ok(result) = tokio::time::timeout(remaining, rx.recv()).await else {
            return None;
        };
        let Ok(event) = result else {
            continue;
        };
        if event.destination != link_id {
            continue;
        }
        if !matches!(event.context, Some(PacketContext::None | PacketContext::LinkClose)) {
            continue;
        }
        let Ok(value) = rmp_serde::from_slice::<rmpv::Value>(event.data.as_slice()) else {
            continue;
        };
        let rmpv::Value::Array(items) = value else {
            continue;
        };
        let Some(signal) = items.first().and_then(|entry| entry.as_u64()) else {
            continue;
        };
        return u8::try_from(signal).ok();
    }
}

fn build_link_request_payload(path: &str, data: rmpv::Value) -> Result<Vec<u8>, std::io::Error> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let path_hash = address_hash(path.as_bytes());
    rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash.to_vec()),
        data,
    ]))
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

async fn send_link_context_packet(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<Link>>,
    context: PacketContext,
    payload: &[u8],
) -> Result<Option<[u8; 16]>, std::io::Error> {
    let packet = {
        let guard = link.lock().await;
        if guard.status() != LinkStatus::Active {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "propagation control link is not active",
            ));
        }

        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload, packet_data.accuire_buf_max())
                .map_err(|_| std::io::Error::other("failed to encrypt link packet"))?;
            ciphertext.len()
        };
        packet_data.resize(cipher_len);

        Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Link,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: *guard.id(),
            transport: None,
            context,
            data: packet_data,
        }
    };

    let request_id = if context == PacketContext::Request {
        let hash = packet.hash().to_bytes();
        let mut request_id = [0u8; 16];
        request_id.copy_from_slice(&hash[..16]);
        Some(request_id)
    } else {
        None
    };

    let outcome = transport.send_packet_with_outcome(packet).await;
    if !send_outcome_is_sent(outcome) {
        return Err(std::io::Error::other(send_outcome_status(
            "propagation control request",
            outcome,
        )));
    }
    Ok(request_id)
}

async fn wait_for_link_request_response(
    data_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    resource_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_destination: AddressHash,
    expected_link_id: AddressHash,
    request_id: [u8; 16],
    timeout: Duration,
) -> Result<rmpv::Value, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err("propagation control response timed out".to_string());
        }
        let remaining = deadline.saturating_duration_since(now);

        tokio::select! {
            _ = tokio::time::sleep(remaining) => {
                return Err("propagation control response timed out".to_string());
            }
            result = data_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.destination != expected_link_id
                            && event.destination != expected_destination
                        {
                            continue;
                        }
                        if let Some((response_id, payload)) = parse_link_response_frame(event.data.as_slice()) {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation control response channel closed".to_string());
                    }
                }
            }
            result = resource_rx.recv() => {
                match result {
                    Ok(event) => {
                        let rns_transport::resource::ResourceEventKind::Complete(complete) = event.kind else {
                            continue;
                        };
                        if event.link_id != expected_link_id {
                            continue;
                        }
                        if let Some((response_id, payload)) = parse_link_response_frame(complete.data.as_slice()) {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation control resource channel closed".to_string());
                    }
                }
            }
        }
    }
}

fn parse_link_response_frame(bytes: &[u8]) -> Option<([u8; 16], rmpv::Value)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 2 {
        return None;
    }
    let request_bytes = value_to_bytes(entries.first()?)?;
    if request_bytes.len() != 16 {
        return None;
    }
    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(request_bytes.as_slice());
    Some((request_id, entries.get(1)?.clone()))
}

fn value_to_bytes(value: &rmpv::Value) -> Option<Vec<u8>> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.clone()),
        rmpv::Value::String(text) => {
            let value = text.as_str()?;
            if let Ok(decoded) = hex::decode(value) {
                return Some(decoded);
            }
            Some(value.as_bytes().to_vec())
        }
        _ => None,
    }
}
