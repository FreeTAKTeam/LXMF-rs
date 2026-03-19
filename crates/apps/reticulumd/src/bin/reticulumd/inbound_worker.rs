use super::bootstrap::PropagationControlContext;
use super::bridge_helpers::{diagnostics_enabled, payload_preview};
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    decode_inbound_payload, decode_inbound_payload_with_diagnostics,
    inbound_stamp_policy_allows_payload,
};
use reticulum_daemon::receipt_bridge::ReceiptEvent;
use rns_rpc::{RpcDaemon, RpcRequest};
use rns_transport::destination::link::{Link, LinkEvent};
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::AddressHash;
use rns_transport::identity::Identity;
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{ReceivedPayloadMode, Transport};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub(super) const OUTBOUND_RESOURCE_SENT_STATUS: &str = "sent: link resource";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OutboundResourceTracking {
    pub(super) message_id: String,
    pub(super) peer: String,
    pub(super) bytes: usize,
    pub(super) sent_status: String,
}

fn inbound_payload_mode(mode: ReceivedPayloadMode) -> InboundPayloadMode {
    match mode {
        ReceivedPayloadMode::FullWire => InboundPayloadMode::FullWire,
        ReceivedPayloadMode::DestinationStripped => InboundPayloadMode::DestinationStripped,
    }
}

pub(super) fn spawn_inbound_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
) {
    if control.enabled {
        spawn_control_worker(daemon.clone(), transport.clone(), control.clone());
    }
    spawn_packet_inbound_worker(daemon.clone(), transport.clone(), control);
    tokio::spawn(async move {
        let mut rx = transport.resource_events();
        loop {
            if let Ok(event) = rx.recv().await {
                match event.kind {
                    ResourceEventKind::Complete(complete) => {
                        if let Some(destination) =
                            resolve_lxmf_resource_destination(transport.as_ref(), &event.link_id)
                                .await
                        {
                            if let Err(error) = inbound_stamp_policy_allows_payload(
                                daemon.as_ref(),
                                destination,
                                &complete.data,
                                InboundPayloadMode::FullWire,
                            ) {
                                if diagnostics_enabled() {
                                    eprintln!(
                                        "[daemon-rx] dropping inbound resource due to stamp policy: {}",
                                        error
                                    );
                                }
                                continue;
                            }
                            if let Some(record) = decode_inbound_payload(
                                destination,
                                &complete.data,
                                InboundPayloadMode::FullWire,
                            ) {
                                let _ = daemon.accept_inbound_with_raw(record, &complete.data);
                            }
                        }
                    }
                    ResourceEventKind::OutboundComplete => {
                        let resource_hash_hex = hex::encode(event.hash.as_slice());
                        if let Some(tracking) = take_outbound_resource_tracking(
                            &outbound_resource_map,
                            resource_hash_hex.as_str(),
                        ) {
                            daemon.record_outbound_peer_activity(
                                &tracking.peer,
                                tracking.bytes,
                                true,
                            );
                            let _ = receipt_tx.send(ReceiptEvent {
                                message_id: tracking.message_id,
                                status: tracking.sent_status,
                            });
                        }
                    }
                    ResourceEventKind::Progress(_) => {}
                }
            }
        }
    });
}

fn spawn_packet_inbound_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
) {
    let daemon_inbound = daemon;
    let inbound_transport = transport;
    tokio::spawn(async move {
        let mut rx = inbound_transport.received_data_events();
        loop {
            if let Ok(event) = rx.recv().await {
                if should_skip_control_payload(&event, &control) {
                    continue;
                }
                let data = event.data.as_slice();
                let destination_hex = hex::encode(event.destination.as_slice());
                if diagnostics_enabled() {
                    eprintln!(
                        "[daemon-rx] dst={} len={} ratchet_used={} data_prefix={}",
                        destination_hex,
                        data.len(),
                        event.ratchet_used,
                        payload_preview(data, 16)
                    );
                }
                let mut destination = [0u8; 16];
                destination.copy_from_slice(event.destination.as_slice());
                let payload_mode = inbound_payload_mode(event.payload_mode);
                let record = if diagnostics_enabled() {
                    let (record, diagnostics) =
                        decode_inbound_payload_with_diagnostics(destination, data, payload_mode);
                    if let Some(ref decoded) = record {
                        eprintln!(
                            "[daemon-rx] decoded msg_id={} src={} dst={} title_len={} content_len={}",
                            decoded.id,
                            decoded.source,
                            decoded.destination,
                            decoded.title.len(),
                            decoded.content.len()
                        );
                    } else {
                        eprintln!(
                            "[daemon-rx] decode-failed dst={} attempts={}",
                            destination_hex,
                            diagnostics.summary()
                        );
                    }
                    record
                } else {
                    decode_inbound_payload(destination, data, payload_mode)
                };
                if record.is_some()
                    && inbound_stamp_policy_allows_payload(
                        daemon_inbound.as_ref(),
                        destination,
                        data,
                        payload_mode,
                    )
                    .is_err()
                {
                    if diagnostics_enabled() {
                        eprintln!(
                            "[daemon-rx] dropping inbound payload due to stamp policy: dst={}",
                            destination_hex
                        );
                    }
                    continue;
                }
                if let Some(record) = record {
                    daemon_inbound.record_inbound_peer_activity(&record.source, data.len());
                    let _ = daemon_inbound.accept_inbound_with_raw(record, data);
                }
            }
        }
    });
}

fn should_skip_control_payload(
    event: &rns_transport::transport::ReceivedData,
    control: &PropagationControlContext,
) -> bool {
    let Some(control_hash) = control.control_destination_hash_hex.as_deref() else {
        return false;
    };
    if hex::encode(event.destination.as_slice()) != control_hash {
        return false;
    }
    matches!(
        event.context,
        Some(PacketContext::Request | PacketContext::Response | PacketContext::LinkIdentify)
    )
}

fn spawn_control_worker(
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    control: PropagationControlContext,
) {
    tokio::spawn(async move {
        let mut rx = transport.in_link_events();
        let identified = Arc::new(Mutex::new(HashMap::<AddressHash, Identity>::new()));
        loop {
            let Ok(event) = rx.recv().await else {
                break;
            };
            let LinkEvent::Data(payload) = event.event else {
                continue;
            };
            let Some(control_hash) = control.control_destination_hash_hex.as_deref() else {
                continue;
            };
            if hex::encode(event.address_hash.as_slice()) != control_hash {
                continue;
            }
            match payload.context() {
                PacketContext::LinkIdentify => {
                    if let Some(identity) =
                        parse_link_identify_payload(payload.as_slice(), &event.id)
                    {
                        if let Ok(mut guard) = identified.lock() {
                            guard.insert(event.id, identity);
                        }
                    }
                }
                PacketContext::Request => {
                    let Some(request_id) = payload.request_id() else {
                        continue;
                    };
                    let remote_identity =
                        identified.lock().ok().and_then(|guard| guard.get(&event.id).cloned());
                    let response = handle_control_request(
                        daemon.as_ref(),
                        &control,
                        payload.as_slice(),
                        remote_identity.as_ref(),
                    );
                    let _ =
                        send_control_response(transport.as_ref(), &event.id, request_id, response)
                            .await;
                }
                _ => {}
            }
        }
    });
}

fn parse_link_identify_payload(payload: &[u8], link_id: &AddressHash) -> Option<Identity> {
    if payload.len() < 32 + 32 + 64 {
        return None;
    }
    let identity = Identity::new_from_slices(&payload[..32], &payload[32..64]);
    let signature = ed25519_dalek::Signature::from_slice(&payload[64..128]).ok()?;
    let mut signed = Vec::with_capacity(16 + 64);
    signed.extend_from_slice(link_id.as_slice());
    signed.extend_from_slice(&payload[..64]);
    identity.verify(&signed, &signature).ok()?;
    Some(identity)
}

fn handle_control_request(
    daemon: &RpcDaemon,
    control: &PropagationControlContext,
    payload: &[u8],
    remote_identity: Option<&Identity>,
) -> ControlResponse {
    const ERROR_NO_IDENTITY: u8 = 0xF0;
    const ERROR_NO_ACCESS: u8 = 0xF1;
    const ERROR_INVALID_DATA: u8 = 0xF4;
    const ERROR_NOT_FOUND: u8 = 0xFD;

    if remote_identity.is_none() {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_IDENTITY);
    }
    let remote_identity = remote_identity.expect("checked above");
    let remote_hash = hex::encode(remote_identity.address_hash.as_slice());
    if !control_identity_allowed(control, &remote_hash) {
        daemon.record_unpeered_propagation_attempt(payload.len());
        return ControlResponse::Code(ERROR_NO_ACCESS);
    }

    let Some((path_hash, data)) = parse_control_request_payload(payload) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
    };
    if path_hash == control_path_hash("/pn/get/stats") {
        return ControlResponse::Value(compose_python_status(daemon, control));
    }

    let Some(peer_hex) = data.and_then(|value| match value {
        rmpv::Value::Binary(bytes) if bytes.len() == 16 => Some(hex::encode(bytes)),
        _ => None,
    }) else {
        return ControlResponse::Code(ERROR_INVALID_DATA);
    };
    let peer_exists = daemon
        .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .and_then(|value| value.get("peers").cloned())
        .and_then(|value| value.as_array().cloned())
        .map(|rows| {
            rows.iter()
                .any(|row| row.get("peer").and_then(Value::as_str) == Some(peer_hex.as_str()))
        })
        .unwrap_or(false);
    if !peer_exists {
        return ControlResponse::Code(ERROR_NOT_FOUND);
    }

    if path_hash == control_path_hash("/pn/peer/sync") {
        let _ = daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "peer_sync".to_string(),
            params: Some(json!({ "peer": peer_hex })),
        });
        return ControlResponse::Bool(true);
    }
    if path_hash == control_path_hash("/pn/peer/unpeer") {
        let _ = daemon.handle_rpc(RpcRequest {
            id: 0,
            method: "peer_unpeer".to_string(),
            params: Some(json!({ "peer": peer_hex })),
        });
        return ControlResponse::Bool(true);
    }

    ControlResponse::Code(ERROR_INVALID_DATA)
}

fn control_identity_allowed(control: &PropagationControlContext, remote_hash: &str) -> bool {
    if control.allowed_control_identities.is_empty() {
        return true;
    }
    control
        .allowed_control_identities
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(remote_hash))
}

fn parse_control_request_payload(payload: &[u8]) -> Option<([u8; 16], Option<rmpv::Value>)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(payload).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 3 {
        return None;
    }
    let path_bytes = match entries.get(1)? {
        rmpv::Value::Binary(bytes) if bytes.len() == 16 => bytes,
        _ => return None,
    };
    let mut path_hash = [0u8; 16];
    path_hash.copy_from_slice(path_bytes.as_slice());
    Some((path_hash, entries.get(2).cloned()))
}

fn control_path_hash(path: &str) -> [u8; 16] {
    let hash = rns_transport::hash::address_hash(path.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(hash.as_slice());
    out
}

fn compose_python_status(daemon: &RpcDaemon, control: &PropagationControlContext) -> Value {
    let status = daemon
        .handle_rpc(RpcRequest { id: 0, method: "daemon_status_ex".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .unwrap_or_else(|| json!({}));
    let peers = daemon
        .handle_rpc(RpcRequest { id: 0, method: "list_peers".to_string(), params: None })
        .ok()
        .and_then(|response| response.result)
        .unwrap_or_else(|| json!({ "peers": [] }));
    let propagation = status.get("propagation").cloned().unwrap_or_else(|| json!({}));
    let stamp_policy = status.get("stamp_policy").cloned().unwrap_or_else(|| json!({}));
    let (message_count, message_bytes) = daemon.message_storage_stats().unwrap_or((0, 0));
    let static_peer_count = propagation
        .get("static_peers")
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    let mut discovered_peer_count = 0_u64;
    let mut total_peer_count = 0_u64;
    let peer_map = peers
        .get("peers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let peer = row.get("peer")?.as_str()?.to_string();
                    let peer_type =
                        row.get("peer_type").and_then(Value::as_str).unwrap_or("discovered");
                    let (outgoing, incoming, offered, unhandled) =
                        daemon.peer_message_stats(peer.as_str()).unwrap_or((0, 0, 0, 0));
                    total_peer_count = total_peer_count.saturating_add(1);
                    if matches!(peer_type, "discovered" | "auto") {
                        discovered_peer_count = discovered_peer_count.saturating_add(1);
                    }
                    Some((
                        peer,
                        json!({
                            "type": peer_type,
                            "state": 0,
                            "alive": row.get("alive").and_then(Value::as_bool).unwrap_or(true),
                            "name": row.get("name").cloned().unwrap_or(Value::Null),
                            "last_heard": row.get("last_seen").and_then(Value::as_i64).unwrap_or(0),
                            "next_sync_attempt": row.get("next_sync_attempt").and_then(Value::as_i64).unwrap_or(0),
                            "last_sync_attempt": row.get("last_sync_attempt").and_then(Value::as_i64).unwrap_or(0),
                            "sync_backoff": row.get("sync_backoff").and_then(Value::as_u64).unwrap_or(0),
                            "peering_timebase": 0,
                            "ler": 0,
                            "str": 0,
                            "transfer_limit": 256,
                            "sync_limit": 10240,
                            "target_stamp_cost": propagation.get("target_cost").and_then(Value::as_u64).unwrap_or(16),
                            "stamp_cost_flexibility": stamp_policy.get("flexibility").and_then(Value::as_u64).unwrap_or(3),
                            "peering_cost": propagation.get("peering_cost").and_then(Value::as_u64).unwrap_or(18),
                            "peering_key": Value::Null,
                            "network_distance": row.get("network_distance").and_then(Value::as_u64).unwrap_or(1),
                            "rx_bytes": row.get("rx_bytes").and_then(Value::as_u64).unwrap_or(0),
                            "tx_bytes": row.get("tx_bytes").and_then(Value::as_u64).unwrap_or(0),
                            "acceptance_rate": row.get("acceptance_rate").and_then(Value::as_f64).unwrap_or(1.0),
                            "messages": {
                                "offered": offered,
                                "outgoing": outgoing,
                                "incoming": incoming,
                                "unhandled": unhandled
                            }
                        }),
                    ))
                })
                .collect::<serde_json::Map<String, Value>>()
        })
        .unwrap_or_default();
    json!({
        "identity_hash": status.get("identity_hash").cloned().unwrap_or(Value::Null),
        "destination_hash": control.propagation_destination_hash_hex.clone().unwrap_or_default(),
        "uptime": SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs(),
        "delivery_limit": 1000,
        "propagation_limit": 256,
        "sync_limit": 10240,
        "target_stamp_cost": propagation.get("target_cost").and_then(Value::as_u64).unwrap_or(16),
        "stamp_cost_flexibility": stamp_policy.get("flexibility").and_then(Value::as_u64).unwrap_or(3),
        "peering_cost": propagation.get("peering_cost").and_then(Value::as_u64).unwrap_or(18),
        "max_peering_cost": propagation.get("remote_peering_cost_max").and_then(Value::as_u64).unwrap_or(26),
        "autopeer_maxdepth": propagation.get("autopeer_maxdepth").and_then(Value::as_u64).unwrap_or(6),
        "from_static_only": propagation.get("from_static_only").and_then(Value::as_bool).unwrap_or(false),
        "messagestore": {
            "count": message_count,
            "bytes": message_bytes,
            "limit": propagation.get("message_storage_limit_mb").and_then(Value::as_u64).map(|value| value * 1024 * 1024),
        },
        "clients": {
            "client_propagation_messages_received": propagation.get("client_propagation_messages_received").and_then(Value::as_u64).unwrap_or(0),
            "client_propagation_messages_served": propagation.get("client_propagation_messages_served").and_then(Value::as_u64).unwrap_or(0),
        },
        "unpeered_propagation_incoming": propagation.get("unpeered_propagation_incoming").and_then(Value::as_u64).unwrap_or(0),
        "unpeered_propagation_rx_bytes": propagation.get("unpeered_propagation_rx_bytes").and_then(Value::as_u64).unwrap_or(0),
        "static_peers": static_peer_count,
        "discovered_peers": discovered_peer_count,
        "total_peers": total_peer_count,
        "max_peers": propagation.get("max_peers").and_then(Value::as_u64).unwrap_or(20),
        "peers": peer_map,
    })
}

enum ControlResponse {
    Code(u8),
    Bool(bool),
    Value(Value),
}

async fn send_control_response(
    transport: &Transport,
    link_id: &AddressHash,
    request_id: [u8; 16],
    response: ControlResponse,
) -> Result<(), std::io::Error> {
    let Some(link) = transport.find_in_link(link_id).await else {
        return Err(std::io::Error::other("control link not found"));
    };
    let response_value = match response {
        ControlResponse::Code(code) => rmpv::Value::from(code),
        ControlResponse::Bool(value) => rmpv::Value::Boolean(value),
        ControlResponse::Value(value) => json_to_rmpv(&value),
    };
    let frame = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.to_vec()), response_value]);
    let payload = rmp_serde::to_vec(&frame).map_err(std::io::Error::other)?;
    let (packet, ingress_iface) = build_link_response_packet(&link, payload.as_slice()).await?;
    let Some(ingress_iface) = ingress_iface else {
        return Err(std::io::Error::other("control link ingress interface unavailable"));
    };
    match packet {
        LinkResponsePacket::Direct(packet) => {
            transport.send_direct(ingress_iface, *packet).await;
            Ok(())
        }
        LinkResponsePacket::Resource(payload) => transport
            .send_resource_direct(link_id, ingress_iface, payload, None)
            .await
            .map(|_| ())
            .map_err(|err| std::io::Error::other(format!("{err:?}"))),
    }
}

enum LinkResponsePacket {
    Direct(Box<Packet>),
    Resource(Vec<u8>),
}

async fn build_link_response_packet(
    link: &Arc<tokio::sync::Mutex<Link>>,
    payload: &[u8],
) -> Result<(LinkResponsePacket, Option<AddressHash>), std::io::Error> {
    let guard = link.lock().await;
    let ingress_iface = guard.ingress_iface();
    let mut packet_data = PacketDataBuffer::new();
    let cipher_len = match guard.encrypt(payload, packet_data.accuire_buf_max()) {
        Ok(ciphertext) => ciphertext.len(),
        Err(_) => return Ok((LinkResponsePacket::Resource(payload.to_vec()), ingress_iface)),
    };
    packet_data.resize(cipher_len);
    Ok((
        LinkResponsePacket::Direct(Box::new(Packet {
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
            context: PacketContext::Response,
            data: packet_data,
        })),
        ingress_iface,
    ))
}

fn json_to_rmpv(value: &Value) -> rmpv::Value {
    match value {
        Value::Null => rmpv::Value::Nil,
        Value::Bool(value) => rmpv::Value::Boolean(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_u64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_f64() {
                rmpv::Value::F64(value)
            } else {
                rmpv::Value::Nil
            }
        }
        Value::String(value) => rmpv::Value::from(value.as_str()),
        Value::Array(values) => rmpv::Value::Array(values.iter().map(json_to_rmpv).collect()),
        Value::Object(map) => rmpv::Value::Map(
            map.iter()
                .map(|(key, value)| (rmpv::Value::from(key.as_str()), json_to_rmpv(value)))
                .collect(),
        ),
    }
}

pub(super) fn track_outbound_resource(
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    resource_hash_hex: String,
    tracking: OutboundResourceTracking,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.insert(resource_hash_hex, tracking);
    }
}

pub(super) fn take_outbound_resource_tracking(
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    resource_hash_hex: &str,
) -> Option<OutboundResourceTracking> {
    outbound_resource_map.lock().ok().and_then(|mut guard| guard.remove(resource_hash_hex))
}

pub(super) fn prune_outbound_resource_mappings_for_message(
    outbound_resource_map: &Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    message_id: &str,
) {
    if let Ok(mut guard) = outbound_resource_map.lock() {
        guard.retain(|_, tracking| tracking.message_id != message_id);
    }
}

async fn resolve_lxmf_resource_destination(
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<[u8; 16]> {
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        if is_lxmf_delivery_destination(guard.destination()) {
            let mut destination = [0u8; 16];
            destination.copy_from_slice(guard.destination().address_hash.as_slice());
            return Some(destination);
        }
        return None;
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        if is_lxmf_delivery_destination(guard.destination()) {
            let mut destination = [0u8; 16];
            destination.copy_from_slice(guard.destination().address_hash.as_slice());
            return Some(destination);
        }
    }
    None
}

fn is_lxmf_delivery_destination(destination: &DestinationDesc) -> bool {
    destination.name.hash == DestinationName::new("lxmf", "delivery").hash
}

#[cfg(test)]
mod tests {
    use super::is_lxmf_delivery_destination;
    use rand_core::OsRng;
    use rns_transport::destination::{DestinationDesc, DestinationName};
    use rns_transport::identity::PrivateIdentity;

    #[test]
    fn lxmf_delivery_destination_is_accepted_for_resource_decode() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *signer.as_identity(),
            address_hash: *signer.address_hash(),
            name: DestinationName::new("lxmf", "delivery"),
        };

        assert!(is_lxmf_delivery_destination(&destination));
    }

    #[test]
    fn non_delivery_destination_is_rejected_for_resource_decode() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *signer.as_identity(),
            address_hash: *signer.address_hash(),
            name: DestinationName::new("lxmf", "propagation.control"),
        };

        assert!(!is_lxmf_delivery_destination(&destination));
    }
}
