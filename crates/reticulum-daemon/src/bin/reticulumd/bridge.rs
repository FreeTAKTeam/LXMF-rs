use super::bridge_helpers::{
    diagnostics_enabled, log_delivery_trace, opportunistic_payload, payload_preview,
    send_trace_detail,
};
use reticulum::delivery::{send_outcome_status, LinkSendResult};
use reticulum::destination::{DestinationDesc, DestinationName, SingleInputDestination};
use reticulum::destination_hash::parse_destination_hash_required;
use reticulum::hash::AddressHash;
use reticulum::identity::{Identity, PrivateIdentity};
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use reticulum::rpc::{AnnounceBridge, OutboundBridge};
use reticulum::transport::{SendPacketOutcome, Transport};
use reticulum_daemon::direct_delivery::send_via_link;
use reticulum_daemon::lxmf_bridge::build_wire_message;
use reticulum_daemon::receipt_bridge::{track_receipt_mapping, ReceiptEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(super) struct TransportBridge {
    transport: Arc<Transport>,
    signer: PrivateIdentity,
    delivery_source_hash: [u8; 16],
    announce_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    announce_app_data: Option<Vec<u8>>,
    peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
    receipt_map: Arc<Mutex<HashMap<String, String>>>,
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
        peer_crypto: Arc<Mutex<HashMap<String, PeerCrypto>>>,
        receipt_map: Arc<Mutex<HashMap<String, String>>>,
        receipt_tx: tokio::sync::mpsc::UnboundedSender<ReceiptEvent>,
    ) -> Self {
        Self {
            transport,
            signer,
            delivery_source_hash,
            announce_destination,
            announce_app_data,
            peer_crypto,
            receipt_map,
            receipt_tx,
        }
    }
}

impl OutboundBridge for TransportBridge {
    fn deliver(
        &self,
        record: &reticulum::storage::messages::MessageRecord,
        _options: &reticulum::rpc::OutboundDeliveryOptions,
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

        let destination_hash = AddressHash::new(destination);
        let transport = self.transport.clone();
        let peer_crypto = self.peer_crypto.clone();
        let receipt_map = self.receipt_map.clone();
        let receipt_tx = self.receipt_tx.clone();
        let message_id = record.id.clone();
        let destination_hex = record.destination.clone();
        tokio::spawn(async move {
            log_delivery_trace(&message_id, &destination_hex, "start", "delivery requested");
            let mut identity = peer_identity;
            // Refresh routing for the destination before link setup.
            transport.request_path(&destination_hash, None, None).await;
            log_delivery_trace(&message_id, &destination_hex, "path-request", "requested");

            if identity.is_none() {
                log_delivery_trace(
                    &message_id,
                    &destination_hex,
                    "identity",
                    "waiting for announce",
                );
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
                let payload_starts_with_dst =
                    payload.len() >= 16 && payload[..16] == destination[..];
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
                    let _ = receipt_tx
                        .send(ReceiptEvent { message_id, status: "sent: link".to_string() });
                }
                Ok(LinkSendResult::Resource(resource_hash)) => {
                    let detail = format!("resource_hash={}", hex::encode(resource_hash.as_slice()));
                    log_delivery_trace(&message_id, &destination_hex, "link", &detail);
                    let _ = receipt_tx.send(ReceiptEvent {
                        message_id,
                        status: "sending: link resource".to_string(),
                    });
                }
                Err(err) => {
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
                        log_delivery_trace(
                            &message_id,
                            &destination_hex,
                            "opportunistic",
                            "sending",
                        );
                    }
                    let trace = transport.send_packet_with_trace(packet).await;
                    let trace_detail = send_trace_detail(trace);
                    log_delivery_trace(
                        &message_id,
                        &destination_hex,
                        "opportunistic",
                        &trace_detail,
                    );
                    let outcome = trace.outcome;
                    if !matches!(
                        outcome,
                        SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast
                    ) {
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
        });
        Ok(())
    }
}

impl AnnounceBridge for TransportBridge {
    fn announce_now(&self) -> Result<(), std::io::Error> {
        let transport = self.transport.clone();
        let destination = self.announce_destination.clone();
        let app_data = self.announce_app_data.clone();
        tokio::spawn(async move {
            transport.send_announce(&destination, app_data.as_deref()).await;
        });
        Ok(())
    }
}
