use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE as BASE64_URL_SAFE};
use base64::Engine as _;
use lxmf::identity;
use lxmf::message::Message;
use lxmf::LxmfError;
use lxmf::{Payload, WireMessage};
use rmpv::Value as RmpValue;
use rns_core::identity::PrivateIdentity;
use serde_json::Value as JsonValue;
use std::io::Cursor;

use crate::lxmf_stamps::FIELD_TICKET;

pub use lxmf::wire_fields::{json_to_rmpv, rmpv_to_json};

const TRANSPORT_FIELDS_MSGPACK_B64_KEY: &str = "_lxmf_fields_msgpack_b64";

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

#[allow(clippy::too_many_arguments)]
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
    build_wire_message_with_options_and_cancel(
        source,
        destination,
        title,
        content,
        fields,
        signer,
        stamp_cost,
        outbound_ticket_hex,
        include_ticket,
        || false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_wire_message_with_options_and_cancel(
    source: [u8; 16],
    destination: [u8; 16],
    title: &str,
    content: &str,
    fields: Option<JsonValue>,
    signer: &PrivateIdentity,
    stamp_cost: Option<u32>,
    outbound_ticket_hex: Option<&str>,
    include_ticket: Option<(i64, &[u8])>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<u8>, LxmfError> {
    let mut message = Message::new();
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    if let Some(fields) = fields {
        message.fields = Some(fields_json_to_rmpv(&fields)?);
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
    let message_id = WireMessage::new(destination, source, payload).try_message_id()?;

    if let Some(ticket_hex) = outbound_ticket_hex {
        let ticket = decode_ticket_hex(ticket_hex)?;
        let stamp = ticket_stamp(&ticket, &message_id);
        message.set_stamp_from_bytes(&stamp);
    } else if let Some(cost) = stamp_cost {
        let stamp = generate_stamp(&message_id, cost, &mut cancelled)
            .ok_or_else(|| LxmfError::Encode("failed to generate LXMF stamp".into()))?;
        message.set_stamp_from_bytes(&stamp);
    }

    let lxmf_signer = identity::PrivateIdentity::from_private_key_bytes(
        &signer.to_private_key_bytes(),
    )
    .map_err(|error| LxmfError::Encode(format!("invalid signer key material: {error:?}")))?;
    message.to_wire(Some(&lxmf_signer))
}

fn fields_json_to_rmpv(fields: &JsonValue) -> Result<RmpValue, LxmfError> {
    if let Some(encoded) = fields
        .as_object()
        .and_then(|object| object.get(TRANSPORT_FIELDS_MSGPACK_B64_KEY))
        .and_then(JsonValue::as_str)
    {
        let bytes = BASE64_STANDARD
            .decode(encoded)
            .or_else(|_| BASE64_URL_SAFE.decode(encoded))
            .map_err(|error| {
                LxmfError::Decode(format!(
                    "invalid {TRANSPORT_FIELDS_MSGPACK_B64_KEY} payload: {error}"
                ))
            })?;
        let mut cursor = Cursor::new(bytes);
        return rmpv::decode::read_value(&mut cursor)
            .map_err(|error| LxmfError::Decode(error.to_string()));
    }
    json_to_rmpv(fields)
}

fn merge_ticket_field(fields: &mut RmpValue, expires_at: i64, ticket: &[u8]) {
    let entry = (
        RmpValue::Integer(FIELD_TICKET.into()),
        RmpValue::Array(vec![
            RmpValue::Integer(expires_at.into()),
            RmpValue::Binary(ticket.to_vec()),
        ]),
    );

    match fields {
        RmpValue::Map(items) => {
            if let Some(existing) = items
                .iter_mut()
                .find(|(key, _)| matches!(key, RmpValue::Integer(value) if value.as_i64() == Some(FIELD_TICKET)))
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
    crate::lxmf_stamps::decode_ticket_hex(ticket_hex).map_err(LxmfError::Encode)
}

fn ticket_stamp(ticket: &[u8], message_id: &[u8; 32]) -> Vec<u8> {
    crate::lxmf_stamps::ticket_stamp(ticket, message_id)
}

fn current_time_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn generate_stamp(
    message_id: &[u8; 32],
    stamp_cost: u32,
    cancelled: &mut impl FnMut() -> bool,
) -> Option<Vec<u8>> {
    crate::lxmf_stamps::generate_stamp_until_cancelled(message_id, stamp_cost, cancelled)
}

pub fn decode_wire_message(bytes: &[u8]) -> Result<Message, LxmfError> {
    Message::from_wire(bytes)
}
