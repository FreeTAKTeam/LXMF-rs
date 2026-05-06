use super::bridge_helpers::{
    diagnostics_enabled, log_delivery_trace, opportunistic_payload, payload_preview,
    send_trace_detail,
};
#[path = "bridge_announce.rs"]
mod announce;
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
        propagation::propagation_link_for_node(
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
            .unwrap_or(propagation::DEFAULT_PROPAGATION_STAMP_COST);
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
        let payload = match propagation::build_propagation_payload(
            &self.payload,
            &destination_identity,
            target_cost,
        ) {
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
        if let Some(link) = propagation::cached_propagation_link(
            &self.outbound_propagation_link,
            propagation_node_hex,
        )
        .await
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

        Ok(propagation::propagation_link_for_node(
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
        let mut identity =
            cached.or_else(|| self.cached_identity_for_destination(destination_hash));
        if identity.is_some() {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(
                &self.message_id,
                detail,
                stage,
                "resolved from cached peer identity",
            );
        }

        if identity.is_none() {
            self.transport.request_path(&destination_hash, None, None).await;
            log_delivery_trace(&self.message_id, &self.destination_hex, stage, "path-requested");
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

fn now_secs_i64() -> i64 {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(i64::MAX)
}
