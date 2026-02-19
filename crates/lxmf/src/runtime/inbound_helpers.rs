use super::wire_codec::json_fields_with_raw_preserved;
use crate::inbound_decode::{decode_inbound_message, InboundPayloadMode};
use crate::message::WireMessage;
use crate::LxmfError;
use rand_core::OsRng;
use reticulum::identity::Identity;
use reticulum::storage::messages::MessageRecord;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn build_propagation_envelope(
    wire_payload: &[u8],
    destination_identity: &Identity,
) -> Result<Vec<u8>, String> {
    let wire = WireMessage::unpack(wire_payload).map_err(|err: LxmfError| err.to_string())?;
    wire.pack_propagation_with_rng(destination_identity, now_epoch_secs() as f64, OsRng)
        .map_err(|err: LxmfError| err.to_string())
}

pub(super) fn decode_inbound_payload(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Option<MessageRecord> {
    let message = decode_inbound_message(destination, payload, mode).ok()?;
    Some(MessageRecord {
        id: message.id,
        source: hex::encode(message.source),
        destination: hex::encode(message.destination),
        title: message.title,
        content: message.content,
        timestamp: message.timestamp,
        direction: "in".into(),
        fields: message.fields.as_ref().and_then(json_fields_with_raw_preserved),
        receipt_status: None,
    })
}

pub(super) fn annotate_inbound_transport_metadata(
    record: &mut MessageRecord,
    event: &reticulum::transport::ReceivedData,
) {
    let mut transport = serde_json::Map::new();
    transport.insert("ratchet_used".to_string(), Value::Bool(event.ratchet_used));

    let mut root = match record.fields.take() {
        Some(Value::Object(existing)) => existing,
        Some(other) => {
            let mut root = serde_json::Map::new();
            root.insert("_fields_raw".to_string(), other);
            root
        }
        None => serde_json::Map::new(),
    };
    root.insert("_transport".to_string(), Value::Object(transport));
    record.fields = Some(Value::Object(root));
}

fn now_epoch_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
