use super::bridge_helpers::{
    diagnostics_enabled, log_delivery_trace, opportunistic_payload, payload_preview,
    send_trace_detail,
};
use super::inbound_worker::{track_outbound_resource, OutboundResourceTracking};
use reticulum_daemon::lxmf_bridge::build_wire_message;
use reticulum_daemon::lxmf_bridge::rmpv_to_json;
use reticulum_daemon::receipt_bridge::{track_receipt_mapping, ReceiptEvent};
use rns_core::identity::PrivateIdentity;
use rns_rpc::{AnnounceBridge, OutboundBridge, RemoteControlBridge, RpcDaemon};
use rns_transport::delivery::await_link_activation;
use rns_transport::delivery::{
    send_outcome_is_sent, send_outcome_status, send_via_link, LinkSendResult,
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
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
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
            receipt_map,
            outbound_resource_map,
            receipt_tx,
        }
    }

    pub(super) fn set_daemon(&self, daemon: Arc<RpcDaemon>) {
        if let Ok(mut guard) = self.daemon.lock() {
            *guard = Some(daemon);
        }
    }
}

struct DeliveryTask {
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
    outbound_resource_map: Arc<Mutex<HashMap<String, OutboundResourceTracking>>>,
    receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    message_id: String,
    destination: [u8; 16],
    destination_hash: AddressHash,
    destination_hex: String,
    payload: Vec<u8>,
    peer_identity: Option<Identity>,
}

impl DeliveryTask {
    async fn run(self) {
        let Self {
            daemon,
            transport,
            peer_crypto,
            receipt_map,
            outbound_resource_map,
            receipt_tx,
            message_id,
            destination,
            destination_hash,
            destination_hex,
            payload,
            peer_identity,
        } = self;

        log_delivery_trace(&message_id, &destination_hex, "start", "delivery requested");
        let mut identity = peer_identity;
        // Refresh routing for the destination before link setup.
        transport.request_path(&destination_hash, None, None).await;
        log_delivery_trace(&message_id, &destination_hex, "path-request", "requested");

        if identity.is_none() {
            log_delivery_trace(&message_id, &destination_hex, "identity", "waiting for announce");
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(12);
            while tokio::time::Instant::now() < deadline {
                if let Some(found) = transport.destination_identity(&destination_hash).await {
                    identity = Some(found);
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }

        let Some(identity) = identity else {
            log_delivery_trace(&message_id, &destination_hex, "identity", "not found");
            let _ = receipt_tx.send(ReceiptEvent {
                message_id,
                status: "failed: peer not announced".to_string(),
            });
            return;
        };
        log_delivery_trace(&message_id, &destination_hex, "identity", "resolved");

        if let Ok(mut peers) = peer_crypto.lock() {
            peers.insert(destination_hex.clone(), PeerCrypto { identity });
        }

        let destination_desc = DestinationDesc {
            identity,
            address_hash: destination_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };

        let result = send_via_link(
            transport.as_ref(),
            destination_desc,
            &payload,
            std::time::Duration::from_secs(20),
        )
        .await;
        if diagnostics_enabled() {
            let payload_starts_with_dst = payload.len() >= 16 && payload[..16] == destination[..];
            let detail = format!(
                "payload_len={} payload_prefix={} starts_with_dst={}",
                payload.len(),
                payload_preview(&payload, 16),
                payload_starts_with_dst
            );
            log_delivery_trace(&message_id, &destination_hex, "payload", &detail);
        }
        match result {
            Ok(LinkSendResult::Packet(packet)) => {
                daemon.record_outbound_peer_activity(&destination_hex, payload.len(), true);
                let packet_hash = hex::encode(packet.hash().to_bytes());
                track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
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
                log_delivery_trace(&message_id, &destination_hex, "link", &detail);
                let _ =
                    receipt_tx.send(ReceiptEvent { message_id, status: "sent: link".to_string() });
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.as_slice());
                track_outbound_resource(
                    &outbound_resource_map,
                    resource_hash_hex.clone(),
                    OutboundResourceTracking {
                        message_id: message_id.clone(),
                        peer: destination_hex.clone(),
                        bytes: payload.len(),
                    },
                );
                let detail = format!("resource_hash={resource_hash_hex}");
                log_delivery_trace(&message_id, &destination_hex, "link", &detail);
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: "sending: link resource".to_string(),
                });
            }
            Err(err) => {
                daemon.record_outbound_peer_activity(&destination_hex, payload.len(), false);
                let err_detail = format!("failed err={err}");
                log_delivery_trace(&message_id, &destination_hex, "link", &err_detail);
                eprintln!(
                    "[daemon] link delivery failed dst={} msg_id={} err={}; trying opportunistic",
                    destination_hex, message_id, err
                );
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id: message_id.clone(),
                    status: format!("link failed: {err}; trying opportunistic"),
                });

                // Opportunistic SINGLE packets must carry LXMF wire bytes
                // without the destination prefix. Receivers prepend the
                // packet destination hash before unpacking.
                let opportunistic_payload = opportunistic_payload(&payload, &destination);
                let mut data = PacketDataBuffer::new();
                if data.write(opportunistic_payload).is_err() {
                    log_delivery_trace(
                        &message_id,
                        &destination_hex,
                        "opportunistic",
                        "payload too large",
                    );
                    let _ = receipt_tx
                        .send(ReceiptEvent { message_id, status: format!("failed: {}", err) });
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
                    destination: destination_hash,
                    transport: None,
                    context: PacketContext::None,
                    data,
                };
                let packet_hash = hex::encode(packet.hash().to_bytes());
                track_receipt_mapping(&receipt_map, &packet_hash, &message_id);
                if diagnostics_enabled() {
                    let detail = format!(
                        "sending packet_hash={} payload_len={} payload_prefix={}",
                        packet_hash,
                        opportunistic_payload.len(),
                        payload_preview(opportunistic_payload, 16)
                    );
                    log_delivery_trace(&message_id, &destination_hex, "opportunistic", &detail);
                } else {
                    log_delivery_trace(&message_id, &destination_hex, "opportunistic", "sending");
                }
                let trace = transport.send_packet_with_trace(packet).await;
                let trace_detail = send_trace_detail(trace);
                log_delivery_trace(&message_id, &destination_hex, "opportunistic", &trace_detail);
                let outcome = trace.outcome;
                if !send_outcome_is_sent(outcome) {
                    if let Ok(mut map) = receipt_map.lock() {
                        map.remove(&packet_hash);
                    }
                }
                let _ = receipt_tx.send(ReceiptEvent {
                    message_id,
                    status: send_outcome_status("opportunistic", outcome),
                });
            }
        }
    }
}

impl OutboundBridge for TransportBridge {
    fn deliver(
        &self,
        record: &rns_rpc::MessageRecord,
        _options: &rns_rpc::OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let destination = parse_destination_hash_required(&record.destination)?;
        let peer_info =
            self.peer_crypto.lock().expect("peer map").get(&record.destination).copied();
        let peer_identity = peer_info.map(|info| info.identity);

        let payload = build_wire_message(
            self.delivery_source_hash,
            destination,
            &record.title,
            &record.content,
            record.fields.clone(),
            &self.signer,
        )
        .map_err(std::io::Error::other)?;

        let daemon = self
            .daemon
            .lock()
            .expect("transport bridge daemon mutex poisoned")
            .clone()
            .ok_or_else(|| std::io::Error::other("daemon bridge unavailable"))?;

        let task = DeliveryTask {
            daemon,
            transport: self.transport.clone(),
            peer_crypto: self.peer_crypto.clone(),
            receipt_map: self.receipt_map.clone(),
            outbound_resource_map: self.outbound_resource_map.clone(),
            receipt_tx: self.receipt_tx.clone(),
            message_id: record.id.clone(),
            destination,
            destination_hash: AddressHash::new(destination),
            destination_hex: record.destination.clone(),
            payload,
            peer_identity,
        };
        tokio::spawn(task.run());
        Ok(())
    }
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

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                remote_control_request(
                    transport.as_ref(),
                    &request_identity,
                    &remote,
                    path,
                    data,
                    timeout,
                )
                .await
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
) -> Result<JsonValue, std::io::Error> {
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

    response_to_json(&response)
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
