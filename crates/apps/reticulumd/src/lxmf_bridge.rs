use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE as BASE64_URL_SAFE};
use base64::Engine as _;
use lxmf::identity;
use lxmf::message::Message;
use lxmf::LxmfError;
use rmpv::Value as RmpValue;
use rns_core::identity::PrivateIdentity;
use serde_json::Value as JsonValue;
use std::io::Cursor;

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
        message.include_ticket(expires_at as f64, ticket);
    }

    message.timestamp = Some(current_time_secs_f64());
    let outbound_ticket = outbound_ticket_hex.map(decode_ticket_hex).transpose()?;
    message.stamp_for_delivery(stamp_cost, outbound_ticket.as_deref(), &mut cancelled)?;

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

fn decode_ticket_hex(ticket_hex: &str) -> Result<Vec<u8>, LxmfError> {
    crate::lxmf_stamps::decode_ticket_hex(ticket_hex).map_err(LxmfError::Encode)
}

fn current_time_secs_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn decode_wire_message(bytes: &[u8]) -> Result<Message, LxmfError> {
    Message::from_wire(bytes)
}
