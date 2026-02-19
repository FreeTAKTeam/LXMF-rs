use lxmf::inbound_decode::{decode_inbound_message, InboundPayloadMode};
use reticulum::storage::messages::MessageRecord;

use crate::lxmf_bridge::rmpv_to_json;

pub fn decode_inbound_payload(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Option<MessageRecord> {
    decode_inbound_payload_with_diagnostics(destination, payload, mode).0
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
    mode: InboundPayloadMode,
) -> (Option<MessageRecord>, InboundDecodeDiagnostics) {
    let mut diagnostics = InboundDecodeDiagnostics::default();
    match decode_inbound_payload_mode(destination, payload, mode) {
        Ok(record) => (Some(record), diagnostics),
        Err(error) => {
            diagnostics.attempts.push(DecodeAttempt {
                candidate: inbound_mode_label(mode),
                len: payload.len(),
                error: error.to_string(),
            });
            (None, diagnostics)
        }
    }
}

fn decode_inbound_payload_mode(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<MessageRecord, lxmf::LxmfError> {
    let message = decode_inbound_message(destination, payload, mode)?;
    Ok(MessageRecord {
        id: message.id,
        source: hex::encode(message.source),
        destination: hex::encode(message.destination),
        title: message.title,
        content: message.content,
        timestamp: message.timestamp,
        direction: "in".into(),
        fields: message.fields.as_ref().and_then(rmpv_to_json),
        receipt_status: None,
    })
}

fn inbound_mode_label(mode: InboundPayloadMode) -> &'static str {
    match mode {
        InboundPayloadMode::FullWire => "full_wire",
        InboundPayloadMode::DestinationStripped => "destination_stripped",
    }
}

#[cfg(test)]
mod tests {
    use super::decode_inbound_payload_with_diagnostics;
    use lxmf::inbound_decode::InboundPayloadMode;

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

        let (record, _) = decode_inbound_payload_with_diagnostics(
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        );
        let record = record.expect("decoded record");
        assert_eq!(record.source, hex::encode(source));
        assert_eq!(record.destination, hex::encode(destination));
        assert_eq!(record.title, "title");
        assert_eq!(record.content, "hello from python-like payload");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.direction, "in");
    }
}
