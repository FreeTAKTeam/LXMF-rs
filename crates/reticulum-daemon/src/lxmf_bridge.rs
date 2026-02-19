use lxmf::message::Message;
use lxmf::LxmfError;
use reticulum::identity::PrivateIdentity;
use serde_json::Value as JsonValue;

pub use lxmf::wire_fields::{json_to_rmpv, rmpv_to_json};

pub fn build_wire_message(
    source: [u8; 16],
    destination: [u8; 16],
    title: &str,
    content: &str,
    fields: Option<JsonValue>,
    signer: &PrivateIdentity,
) -> Result<Vec<u8>, LxmfError> {
    let mut message = Message::new();
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    if let Some(fields) = fields {
        message.fields = Some(json_to_rmpv(&fields)?);
    }
    message.to_wire(Some(signer))
}

pub fn decode_wire_message(bytes: &[u8]) -> Result<Message, LxmfError> {
    Message::from_wire(bytes)
}
