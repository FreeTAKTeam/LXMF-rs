use crate::message::Message;
use crate::LxmfError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundPayloadMode {
    FullWire,
    DestinationStripped,
}

#[derive(Debug, Clone)]
pub struct DecodedInboundMessage {
    pub id: String,
    pub source: [u8; 16],
    pub destination: [u8; 16],
    pub title: String,
    pub content: String,
    pub timestamp: i64,
    pub fields: Option<rmpv::Value>,
}

pub fn decode_inbound_message(
    fallback_destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<DecodedInboundMessage, LxmfError> {
    let wire = match mode {
        InboundPayloadMode::FullWire => payload.to_vec(),
        InboundPayloadMode::DestinationStripped => {
            let mut with_destination_prefix = Vec::with_capacity(16 + payload.len());
            with_destination_prefix.extend_from_slice(&fallback_destination);
            with_destination_prefix.extend_from_slice(payload);
            with_destination_prefix
        }
    };

    let message = Message::from_wire(&wire)?;
    let source = message.source_hash.unwrap_or([0u8; 16]);
    let destination = message.destination_hash.unwrap_or(fallback_destination);
    let id = wire_message_id_hex(&wire).unwrap_or_else(|| hex::encode(destination));
    Ok(DecodedInboundMessage {
        id,
        source,
        destination,
        title: String::from_utf8(message.title).unwrap_or_default(),
        content: String::from_utf8(message.content).unwrap_or_default(),
        timestamp: message.timestamp.map(|value| value as i64).unwrap_or(0),
        fields: message.fields,
    })
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
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(destination);
    hasher.update(source);
    hasher.update(payload_without_stamp);
    hex::encode(hasher.finalize())
}
