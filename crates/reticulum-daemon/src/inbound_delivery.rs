use reticulum::storage::messages::MessageRecord;
use sha2::{Digest, Sha256};

use crate::lxmf_bridge::{decode_wire_message, rmpv_to_json};

pub fn decode_inbound_payload(destination: [u8; 16], payload: &[u8]) -> Option<MessageRecord> {
    decode_inbound_payload_with_diagnostics(destination, payload).0
}

#[derive(Debug, Clone)]
pub struct DecodeAttempt {
    pub candidate: &'static str,
    pub len: usize,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct InboundDecodeDiagnostics {
    pub attempts: Vec<DecodeAttempt>,
}

impl InboundDecodeDiagnostics {
    pub fn summary(&self) -> String {
        if self.attempts.is_empty() {
            return "no decode attempts".to_string();
        }
        self.attempts
            .iter()
            .map(|attempt| format!("{}(len={}):{}", attempt.candidate, attempt.len, attempt.error))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

pub fn decode_inbound_payload_with_diagnostics(
    destination: [u8; 16],
    payload: &[u8],
) -> (Option<MessageRecord>, InboundDecodeDiagnostics) {
    let mut decode_candidates: Vec<(&'static str, Vec<u8>)> = Vec::with_capacity(3);
    decode_candidates.push(("raw", payload.to_vec()));

    let mut with_destination_prefix = Vec::with_capacity(16 + payload.len());
    with_destination_prefix.extend_from_slice(&destination);
    with_destination_prefix.extend_from_slice(payload);
    decode_candidates.push(("dst_prefix+raw", with_destination_prefix));

    if payload.len() > 16 && payload[..16] == destination {
        decode_candidates.push(("raw_without_dst_prefix", payload[16..].to_vec()));
    }

    let mut diagnostics = InboundDecodeDiagnostics::default();
    for (label, candidate) in decode_candidates {
        match decode_wire_candidate(destination, &candidate) {
            Some(record) => return (Some(record), diagnostics),
            None => {
                let err = decode_wire_message(&candidate)
                    .err()
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unrecognized wire payload".to_string());
                diagnostics.attempts.push(DecodeAttempt {
                    candidate: label,
                    len: candidate.len(),
                    error: err,
                });
            }
        }
    }

    (None, diagnostics)
}

fn decode_wire_candidate(
    fallback_destination: [u8; 16],
    candidate: &[u8],
) -> Option<MessageRecord> {
    if let Ok(message) = decode_wire_message(candidate) {
        let source = message.source_hash.unwrap_or([0u8; 16]);
        let destination = message.destination_hash.unwrap_or(fallback_destination);
        let id = wire_message_id_hex(candidate).unwrap_or_else(|| hex::encode(destination));
        return Some(MessageRecord {
            id,
            source: hex::encode(source),
            destination: hex::encode(destination),
            title: String::from_utf8(message.title).unwrap_or_default(),
            content: String::from_utf8(message.content).unwrap_or_default(),
            timestamp: message.timestamp.map(|v| v as i64).unwrap_or(0),
            direction: "in".into(),
            fields: message.fields.as_ref().and_then(rmpv_to_json),
            receipt_status: None,
        });
    }

    let decoded = decode_wire_candidate_relaxed(candidate)?;
    Some(MessageRecord {
        id: decoded.id,
        source: hex::encode(decoded.source),
        destination: hex::encode(decoded.destination),
        title: decoded.title,
        content: decoded.content,
        timestamp: decoded.timestamp,
        direction: "in".into(),
        fields: decoded.fields.as_ref().and_then(rmpv_to_json),
        receipt_status: None,
    })
}

struct RelaxedInboundMessage {
    id: String,
    source: [u8; 16],
    destination: [u8; 16],
    title: String,
    content: String,
    timestamp: i64,
    fields: Option<rmpv::Value>,
}

fn decode_wire_candidate_relaxed(candidate: &[u8]) -> Option<RelaxedInboundMessage> {
    // LXMF wire: 16-byte destination + 16-byte source + 64-byte signature + msgpack payload.
    const SIGNATURE_LEN: usize = 64;
    const HEADER_LEN: usize = 16 + 16 + SIGNATURE_LEN;
    if candidate.len() <= HEADER_LEN {
        return None;
    }

    let mut destination = [0u8; 16];
    destination.copy_from_slice(&candidate[..16]);
    let mut source = [0u8; 16];
    source.copy_from_slice(&candidate[16..32]);
    let payload = &candidate[HEADER_LEN..];
    let payload_value = rmp_serde::from_slice::<rmpv::Value>(payload).ok()?;
    let rmpv::Value::Array(items) = payload_value else {
        return None;
    };
    if items.len() < 4 || items.len() > 5 {
        return None;
    }

    let timestamp = parse_payload_timestamp(items.first()?)? as i64;
    let title = decode_payload_text(items.get(1));
    let content = decode_payload_text(items.get(2));
    let fields = match items.get(3) {
        Some(rmpv::Value::Nil) | None => None,
        Some(value) => Some(value.clone()),
    };

    let payload_without_stamp = payload_without_stamp_bytes(&items)?;
    let id = compute_message_id_hex(destination, source, &payload_without_stamp);

    Some(RelaxedInboundMessage { id, source, destination, title, content, timestamp, fields })
}

fn parse_payload_timestamp(value: &rmpv::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|v| v as f64))
        .or_else(|| value.as_u64().map(|v| v as f64))
}

fn decode_payload_text(value: Option<&rmpv::Value>) -> String {
    match value {
        Some(rmpv::Value::Binary(bytes)) => String::from_utf8(bytes.clone()).unwrap_or_default(),
        Some(rmpv::Value::String(text)) => text.as_str().map(ToOwned::to_owned).unwrap_or_default(),
        _ => String::new(),
    }
}

fn wire_message_id_hex(candidate: &[u8]) -> Option<String> {
    const SIGNATURE_LEN: usize = 64;
    const HEADER_LEN: usize = 16 + 16 + SIGNATURE_LEN;
    if candidate.len() <= HEADER_LEN {
        return None;
    }
    let mut destination = [0u8; 16];
    destination.copy_from_slice(&candidate[..16]);
    let mut source = [0u8; 16];
    source.copy_from_slice(&candidate[16..32]);
    let payload_value = rmp_serde::from_slice::<rmpv::Value>(&candidate[HEADER_LEN..]).ok()?;
    let rmpv::Value::Array(items) = payload_value else {
        return None;
    };
    let payload_without_stamp = payload_without_stamp_bytes(&items)?;
    Some(compute_message_id_hex(destination, source, &payload_without_stamp))
}

fn payload_without_stamp_bytes(items: &[rmpv::Value]) -> Option<Vec<u8>> {
    if items.len() < 4 || items.len() > 5 {
        return None;
    }
    let mut trimmed = items.to_vec();
    if trimmed.len() == 5 {
        trimmed.pop();
    }
    rmp_serde::to_vec(&rmpv::Value::Array(trimmed)).ok()
}

fn compute_message_id_hex(
    destination: [u8; 16],
    source: [u8; 16],
    payload_without_stamp: &[u8],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(destination);
    hasher.update(source);
    hasher.update(payload_without_stamp);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::decode_inbound_payload_with_diagnostics;

    #[test]
    fn decode_inbound_payload_accepts_integer_timestamp_wire() {
        let destination = [0x11; 16];
        let source = [0x22; 16];
        let signature = [0x33; 64];
        let payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
            rmpv::Value::from(1_770_000_000_i64),
            rmpv::Value::from("title"),
            rmpv::Value::from("hello from python-like payload"),
            rmpv::Value::Nil,
        ]))
        .expect("payload encoding");
        let mut wire = Vec::new();
        wire.extend_from_slice(&destination);
        wire.extend_from_slice(&source);
        wire.extend_from_slice(&signature);
        wire.extend_from_slice(&payload);

        let (record, _) = decode_inbound_payload_with_diagnostics(destination, &wire);
        let record = record.expect("decoded record");
        assert_eq!(record.source, hex::encode(source));
        assert_eq!(record.destination, hex::encode(destination));
        assert_eq!(record.title, "title");
        assert_eq!(record.content, "hello from python-like payload");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.direction, "in");
    }
}
