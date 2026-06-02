use std::path::Path;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use lxmf::{Message, WireMessage};
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::lxmf_bridge::rmpv_to_json;

#[derive(Debug, Clone, Serialize)]
pub struct InterchangeMessageSummary {
    pub message_id: String,
    pub source: String,
    pub destination: String,
    pub timestamp_f64: f64,
    pub title_utf8: Option<String>,
    pub content_utf8: Option<String>,
    pub title_base64: String,
    pub content_base64: String,
    pub fields: Option<JsonValue>,
    pub stamp_base64: Option<String>,
}

pub fn decode_storage_file(
    path: impl AsRef<Path>,
) -> Result<InterchangeMessageSummary, lxmf::LxmfError> {
    let wire = WireMessage::unpack_storage_from_file(path)?;
    summarize_wire_message(&wire)
}

pub fn summarize_wire_message(
    wire: &WireMessage,
) -> Result<InterchangeMessageSummary, lxmf::LxmfError> {
    let packed = wire.pack()?;
    let message = Message::from_wire(&packed)?;
    let title_utf8 = String::from_utf8(message.title.clone()).ok();
    let content_utf8 = String::from_utf8(message.content.clone()).ok();
    let fields = message.fields.as_ref().and_then(rmpv_to_json);

    Ok(InterchangeMessageSummary {
        message_id: hex::encode(wire.message_id()),
        source: hex::encode(wire.source),
        destination: hex::encode(wire.destination),
        timestamp_f64: message.timestamp.unwrap_or(0.0),
        title_utf8,
        content_utf8,
        title_base64: BASE64_STANDARD.encode(&message.title),
        content_base64: BASE64_STANDARD.encode(&message.content),
        fields,
        stamp_base64: message.stamp.as_ref().map(|stamp| BASE64_STANDARD.encode(stamp)),
    })
}

#[cfg(test)]
mod tests {
    use super::decode_storage_file;
    use crate::lxmf_bridge::build_wire_message;
    use rns_core::identity::PrivateIdentity;
    use std::fs;

    #[test]
    fn decode_storage_file_accepts_python_style_container() {
        let sender = PrivateIdentity::new_from_name("paper-interchange-sender");
        let receiver = PrivateIdentity::new_from_name("paper-interchange-receiver");
        let mut source = [0u8; 16];
        source.copy_from_slice(sender.address_hash().as_slice());
        let mut destination = [0u8; 16];
        destination.copy_from_slice(receiver.address_hash().as_slice());
        let wire = build_wire_message(source, destination, "title", "content", None, &sender)
            .expect("wire");
        let container = rmp_serde::to_vec_named(&rmpv::Value::Map(vec![
            (rmpv::Value::String("state".into()), rmpv::Value::Integer(4_i64.into())),
            (rmpv::Value::String("lxmf_bytes".into()), rmpv::Value::Binary(wire.clone())),
            (rmpv::Value::String("transport_encrypted".into()), rmpv::Value::Boolean(true)),
            (
                rmpv::Value::String("transport_encryption".into()),
                rmpv::Value::String("Curve25519".into()),
            ),
            (rmpv::Value::String("method".into()), rmpv::Value::Integer(2_i64.into())),
        ]))
        .expect("container");
        let temp = tempfile::NamedTempFile::new().expect("temp");
        fs::write(temp.path(), container).expect("write");

        let summary = decode_storage_file(temp.path()).expect("decode");
        assert_eq!(summary.title_utf8.as_deref(), Some("title"));
        assert_eq!(summary.content_utf8.as_deref(), Some("content"));
        assert_eq!(summary.source, hex::encode(source));
        assert_eq!(summary.destination, hex::encode(destination));
    }
}
