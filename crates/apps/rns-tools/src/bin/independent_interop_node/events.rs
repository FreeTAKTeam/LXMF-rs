use super::model::{address_hex, SharedState, CHANNEL_MESSAGE_TYPE};
use super::node::request_envelope_details;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rns_transport::destination::link::{LinkEvent, LinkEventData};
use rns_transport::packet::PacketContext;
use rns_transport::resource::{ResourceEvent, ResourceEventKind};
use rns_transport::transport::{DeliveryReceipt, ReceiptHandler, ReceivedData, Transport};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub fn receipt_handler(state: SharedState) -> Box<dyn ReceiptHandler> {
    Box::new(ReceiptRecorder { state })
}

pub fn spawn(transport: Arc<Transport>, state: SharedState) {
    spawn_event_collectors(transport, state);
}

struct ReceiptRecorder {
    state: SharedState,
}

impl ReceiptHandler for ReceiptRecorder {
    fn on_receipt(&self, receipt: &DeliveryReceipt) {
        let state = self.state.clone();
        let packet_hash = hex::encode(receipt.packet_hash().as_slice());
        tokio::spawn(async move {
            state.record(json!({"type": "receipt", "packet_hash": packet_hash})).await;
        });
    }
}

fn spawn_event_collectors(transport: Arc<Transport>, state: SharedState) {
    spawn_announce_collector(transport.clone(), state.clone());
    spawn_data_collector(transport.clone(), state.clone());
    spawn_link_collector(transport.clone(), state.clone(), true);
    spawn_link_collector(transport.clone(), state.clone(), false);
    spawn_resource_collector(transport, state);
}

fn spawn_announce_collector(transport: Arc<Transport>, state: SharedState) {
    tokio::spawn(async move {
        let mut events = transport.recv_announces().await;
        while let Ok(event) = events.recv().await {
            let description = event.destination.lock().await.desc;
            state.known_destinations.write().await.insert(description.address_hash, description);
            state
                .record(json!({
                    "type": "announce",
                    "destination_hash": address_hex(&description.address_hash),
                    "app_data": BASE64.encode(event.app_data.as_slice()),
                    "hops": event.hops,
                    "name_hash": hex::encode(event.name_hash),
                    "signature_verified": true,
                }))
                .await;
        }
    });
}

fn spawn_data_collector(transport: Arc<Transport>, state: SharedState) {
    tokio::spawn(async move {
        let mut events = transport.received_data_events();
        while let Ok(event) = events.recv().await {
            record_received_data(&state, event).await;
        }
    });
}

async fn record_received_data(state: &SharedState, event: ReceivedData) {
    let envelope = request_envelope_details(event.context, event.data.as_slice());
    state
        .record(json!({
            "type": "data",
            "destination_hash": address_hex(&event.destination),
            "data": BASE64.encode(event.data.as_slice()),
            "sha256": hex::encode(Sha256::digest(event.data.as_slice())),
            "bytes": event.data.len(),
            "context": event.context.map(|value| value as u8),
            "request_id": event.request_id.map(hex::encode),
            "request_path_hash": envelope.as_ref().and_then(|value| value.0.clone()),
            "application_data": envelope.as_ref().and_then(|value| value.1.clone()),
            "hops": event.hops,
        }))
        .await;
}

fn spawn_link_collector(transport: Arc<Transport>, state: SharedState, outbound: bool) {
    let mut events =
        if outbound { transport.out_link_events() } else { transport.in_link_events() };
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            record_link_event(&transport, &state, event, outbound).await;
        }
    });
}

async fn record_link_event(
    transport: &Transport,
    state: &SharedState,
    event: LinkEventData,
    outbound: bool,
) {
    let event_name = match &event.event {
        LinkEvent::Activated => "activated",
        LinkEvent::Data(_) => "data",
        LinkEvent::PeerIdentified(_) => "peer_identified",
        LinkEvent::Closed => "closed",
    };
    let snapshot = json!({
        "link_id": address_hex(&event.id),
        "destination_hash": address_hex(&event.address_hash),
        "direction": if outbound { "outbound" } else { "inbound" },
        "state": event_name,
    });
    state.links.write().await.insert(event.id, snapshot.clone());
    state.record(json!({"type": "link", "event": snapshot})).await;
    if matches!(event.event, LinkEvent::Activated) {
        let state = state.clone();
        let _ = transport
            .channel(event.id)
            .register_handler(CHANNEL_MESSAGE_TYPE, move |envelope| {
                let state = state.clone();
                let payload = envelope.payload;
                tokio::spawn(async move {
                    state
                        .record(json!({
                            "type": "channel",
                            "message_type": CHANNEL_MESSAGE_TYPE,
                            "sequence": envelope.sequence,
                            "payload": BASE64.encode(&payload),
                            "sha256": hex::encode(Sha256::digest(&payload)),
                        }))
                        .await;
                });
                true
            })
            .await;
    }
}

fn spawn_resource_collector(transport: Arc<Transport>, state: SharedState) {
    tokio::spawn(async move {
        let mut events = transport.resource_events();
        while let Ok(event) = events.recv().await {
            record_resource_event(&state, event).await;
        }
    });
}

async fn record_resource_event(state: &SharedState, event: ResourceEvent) {
    let details = match event.kind {
        ResourceEventKind::Progress(progress) => json!({
            "state": "progress",
            "received_bytes": progress.received_bytes,
            "total_bytes": progress.total_bytes,
        }),
        ResourceEventKind::SegmentComplete(progress) => json!({
            "state": "segment_complete",
            "segment_index": progress.segment_index,
            "total_segments": progress.total_segments,
        }),
        ResourceEventKind::Complete(complete) => {
            let application = if complete.is_response {
                request_envelope_details(Some(PacketContext::Response), &complete.data)
                    .and_then(|value| value.1)
                    .and_then(|value| BASE64.decode(value).ok())
            } else {
                None
            };
            json!({
                "state": "complete",
                "bytes": complete.data.len(),
                "sha256": hex::encode(Sha256::digest(&complete.data)),
                "application_bytes": application.as_ref().map(Vec::len),
                "application_sha256": application
                    .as_ref()
                    .map(|value| hex::encode(Sha256::digest(value))),
                "metadata": complete.metadata.map(|value| BASE64.encode(value)),
                "request_id": complete.request_id.map(hex::encode),
                "is_request": complete.is_request,
                "is_response": complete.is_response,
            })
        }
        ResourceEventKind::InboundFailed(failure) => json!({
            "state": "inbound_failed",
            "reason": failure.reason,
            "received_bytes": failure.progress.received_bytes,
            "total_bytes": failure.progress.total_bytes,
        }),
        ResourceEventKind::OutboundComplete => json!({"state": "outbound_complete"}),
        ResourceEventKind::OutboundFailed => json!({"state": "outbound_failed"}),
        ResourceEventKind::OutboundCancelled => json!({"state": "outbound_cancelled"}),
    };
    state
        .record(json!({
            "type": "resource",
            "resource_hash": hex::encode(event.hash.as_slice()),
            "link_id": address_hex(&event.link_id),
            "details": details,
        }))
        .await;
}
