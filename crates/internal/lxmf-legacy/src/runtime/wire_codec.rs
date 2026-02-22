use crate::message::Message;
use crate::payload_fields::decode_transport_fields_json;
use crate::wire_fields::{
    json_to_rmpv as json_to_rmpv_shared, rmpv_to_json_with_options, RmpvToJsonOptions,
};
use crate::LxmfError;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use reticulum::identity::PrivateIdentity;
use serde_json::Value;

pub(super) fn build_wire_message(
    source: [u8; 16],
    destination: [u8; 16],
    title: &str,
    content: &str,
    fields: Option<Value>,
    signer: &PrivateIdentity,
) -> Result<Vec<u8>, LxmfError> {
    let mut message = Message::new();
    message.destination_hash = Some(destination);
    message.source_hash = Some(source);
    message.set_title_from_string(title);
    message.set_content_from_string(content);
    if let Some(fields) = fields {
        message.fields = Some(wire_fields_from_json(&fields)?);
    }
    message.to_wire(Some(signer))
}

fn wire_fields_from_json(value: &Value) -> Result<rmpv::Value, LxmfError> {
    if let Some(raw) = decode_transport_fields_json(value)? {
        return Ok(raw);
    }
    json_to_rmpv(value)
}

pub(super) fn json_to_rmpv(value: &Value) -> Result<rmpv::Value, LxmfError> {
    json_to_rmpv_shared(value)
}

pub(super) fn rmpv_to_json(value: &rmpv::Value) -> Option<Value> {
    rmpv_to_json_with_options(value, RmpvToJsonOptions { enrich_app_extensions: true })
}

pub(super) fn json_fields_with_raw_preserved(value: &rmpv::Value) -> Option<Value> {
    let mut converted = rmpv_to_json(value)?;
    if let Value::Object(object) = &mut converted {
        if let Ok(raw) = rmp_serde::to_vec(value) {
            object.insert(
                "_lxmf_fields_msgpack_b64".to_string(),
                Value::String(BASE64_STANDARD.encode(raw)),
            );
        }
    }
    Some(converted)
}

pub(super) fn sanitize_outbound_wire_fields(fields: Option<&Value>) -> Option<Value> {
    let Some(Value::Object(fields)) = fields else {
        return fields.cloned();
    };

    let mut out = fields.clone();
    out.remove(super::OUTBOUND_DELIVERY_OPTIONS_FIELD);

    for key in ["_lxmf", "lxmf"] {
        let Some(Value::Object(lxmf_fields)) = out.get_mut(key) else {
            continue;
        };
        for reserved in [
            "method",
            "stamp_cost",
            "include_ticket",
            "try_propagation_on_fail",
            "source_private_key",
        ] {
            lxmf_fields.remove(reserved);
        }
        if lxmf_fields.is_empty() {
            out.remove(key);
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}
