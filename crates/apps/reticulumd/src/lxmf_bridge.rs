use hkdf::Hkdf;
use lxmf::identity;
use lxmf::message::Message;
use lxmf::LxmfError;
use lxmf::{Payload, WireMessage};
use rmpv::Value as RmpValue;
use rns_core::hash::address_hash;
use rns_core::identity::PrivateIdentity;
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const FIELD_TICKET: i64 = 0x0C;
const TICKET_LENGTH: usize = 16;
const WORKBLOCK_EXPAND_ROUNDS: usize = 3000;

pub use lxmf::wire_fields::{json_to_rmpv, rmpv_to_json};

pub fn build_wire_message(
    source: [u8; 16],
    destination: [u8; 16],
    title: &str,
    content: &str,
    fields: Option<JsonValue>,
    signer: &PrivateIdentity,
) -> Result<Vec<u8>, LxmfError> {
    build_wire_message_with_options(
        source,
        destination,
        title,
        content,
        fields,
        signer,
        None,
        None,
        None,
    )
}

pub fn build_wire_message_with_options(
    source: [u8; 16],
    destination: [u8; 16],
    title: &str,
    content: &str,
    fields: Option<JsonValue>,
    signer: &PrivateIdentity,
    stamp_cost: Option<u32>,
    outbound_ticket_hex: Option<&str>,
    include_ticket: Option<(i64, &[u8])>,
) -> Result<Vec<u8>, LxmfError> {
    let mut message = Message::new();
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    if let Some(fields) = fields {
        message.fields = Some(json_to_rmpv(&fields)?);
    }
    if let Some((expires_at, ticket)) = include_ticket {
        let fields = message.fields.get_or_insert_with(|| RmpValue::Map(Vec::new()));
        merge_ticket_field(fields, expires_at, ticket);
    }

    let timestamp = message.timestamp.unwrap_or_else(current_time_secs_f64);
    message.timestamp = Some(timestamp);
    let payload = Payload::new(
        timestamp,
        Some(message.content.clone()),
        Some(message.title.clone()),
        message.fields.clone(),
        None,
    );
    let message_id = WireMessage::new(destination, source, payload).message_id();

    if let Some(ticket_hex) = outbound_ticket_hex {
        let ticket = decode_ticket_hex(ticket_hex)?;
        let stamp = ticket_stamp(&ticket, &message_id);
        message.set_stamp_from_bytes(&stamp);
    } else if let Some(cost) = stamp_cost {
        let stamp = generate_stamp(&message_id, cost)
            .ok_or_else(|| LxmfError::Encode("failed to generate LXMF stamp".into()))?;
        message.set_stamp_from_bytes(&stamp);
    }

    let lxmf_signer = identity::PrivateIdentity::from_private_key_bytes(
        &signer.to_private_key_bytes(),
    )
    .map_err(|error| LxmfError::Encode(format!("invalid signer key material: {error:?}")))?;
    message.to_wire(Some(&lxmf_signer))
}

fn merge_ticket_field(fields: &mut RmpValue, expires_at: i64, ticket: &[u8]) {
    let entry = (
        RmpValue::Integer(i64::from(FIELD_TICKET).into()),
        RmpValue::Array(vec![
            RmpValue::Integer(expires_at.into()),
            RmpValue::Binary(ticket.to_vec()),
        ]),
    );

    match fields {
        RmpValue::Map(items) => {
            if let Some(existing) = items
                .iter_mut()
                .find(|(key, _)| matches!(key, RmpValue::Integer(value) if value.as_i64() == Some(i64::from(FIELD_TICKET))))
            {
                existing.1 = entry.1;
            } else {
                items.push(entry);
            }
        }
        other => {
            *other = RmpValue::Map(vec![entry]);
        }
    }
}

fn decode_ticket_hex(ticket_hex: &str) -> Result<Vec<u8>, LxmfError> {
    let bytes = hex::decode(ticket_hex.trim())
        .map_err(|error| LxmfError::Encode(format!("invalid outbound ticket hex: {error}")))?;
    if bytes.len() != TICKET_LENGTH {
        return Err(LxmfError::Encode(format!(
            "invalid outbound ticket length {}; expected {} bytes",
            bytes.len(),
            TICKET_LENGTH
        )));
    }
    Ok(bytes)
}

fn ticket_stamp(ticket: &[u8], message_id: &[u8; 32]) -> Vec<u8> {
    let mut material = Vec::with_capacity(ticket.len() + message_id.len());
    material.extend_from_slice(ticket);
    material.extend_from_slice(message_id);
    address_hash(&material).to_vec()
}

fn current_time_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn generate_stamp(message_id: &[u8; 32], stamp_cost: u32) -> Option<Vec<u8>> {
    let workblock = stamp_workblock(message_id, WORKBLOCK_EXPAND_ROUNDS);
    let mut nonce = 0u64;
    loop {
        let stamp = nonce.to_le_bytes().to_vec();
        if stamp_valid(&stamp, stamp_cost, &workblock) {
            return Some(stamp);
        }
        nonce = nonce.wrapping_add(1);
        if nonce == 0 {
            return None;
        }
    }
}

fn stamp_workblock(material: &[u8], expand_rounds: usize) -> Vec<u8> {
    let mut workblock = Vec::with_capacity(expand_rounds * 256);
    for n in 0..expand_rounds {
        let mut salt_data = Vec::with_capacity(material.len() + 8);
        salt_data.extend_from_slice(material);
        let packed = rmp_serde::to_vec(&n).unwrap_or_default();
        salt_data.extend_from_slice(&packed);
        let salt_hash = Sha256::digest(&salt_data);
        let hk = Hkdf::<Sha256>::new(Some(salt_hash.as_slice()), material);
        let mut okm = [0u8; 256];
        hk.expand(&[], &mut okm).expect("hkdf expand for LXMF stamp workblock");
        workblock.extend_from_slice(&okm);
    }
    workblock
}

fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    stamp_value(workblock, stamp) >= target_cost
}

fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let mut material = Vec::with_capacity(workblock.len() + stamp.len());
    material.extend_from_slice(workblock);
    material.extend_from_slice(stamp);
    let hash = Sha256::digest(&material);
    let mut value = 0u32;
    for byte in hash {
        if byte == 0 {
            value += 8;
        } else {
            value += byte.leading_zeros();
            break;
        }
    }
    value
}

pub fn decode_wire_message(bytes: &[u8]) -> Result<Message, LxmfError> {
    Message::from_wire(bytes)
}
