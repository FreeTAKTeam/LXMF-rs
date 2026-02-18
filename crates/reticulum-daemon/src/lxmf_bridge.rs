use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use lxmf::error::LxmfError;
use lxmf::message::Message;
use reticulum::identity::PrivateIdentity;
use rmpv::Value;
use serde_json::Value as JsonValue;

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
        let mut fields = fields;
        normalize_attachment_fields_for_wire(&mut fields);
        message.fields = Some(json_to_rmpv(&fields)?);
    }
    message.to_wire(Some(signer))
}

pub fn decode_wire_message(bytes: &[u8]) -> Result<Message, LxmfError> {
    Message::from_wire(bytes)
}

pub fn json_to_rmpv(value: &JsonValue) -> Result<Value, LxmfError> {
    json_to_rmpv_lossless(value)
}

fn normalize_attachment_fields_for_wire(fields: &mut JsonValue) {
    let JsonValue::Object(map) = fields else {
        return;
    };

    let normalized_field_5 = map
        .get("5")
        .and_then(JsonValue::as_array)
        .and_then(|entries| normalize_file_attachments(entries))
        .or_else(|| {
            map.get("attachments")
                .and_then(JsonValue::as_array)
                .and_then(|entries| normalize_file_attachments(entries))
        })
        .or_else(|| {
            map.get("files")
                .and_then(JsonValue::as_array)
                .and_then(|entries| normalize_file_attachments(entries))
        });
    if let Some(value) = normalized_field_5 {
        map.insert("5".to_string(), value);
        map.remove("attachments");
        map.remove("files");
        return;
    }

    map.remove("5");
}

fn normalize_file_attachments(entries: &[JsonValue]) -> Option<JsonValue> {
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(value) = normalize_file_attachment_entry(entry) {
            normalized.push(value);
        }
    }
    if normalized.is_empty() {
        None
    } else {
        Some(JsonValue::Array(normalized))
    }
}

fn normalize_file_attachment_entry(entry: &JsonValue) -> Option<JsonValue> {
    match entry {
        JsonValue::Array(items) if items.len() >= 2 => {
            let filename = items[0].as_str()?;
            let data = normalize_attachment_data(&items[1])?;
            Some(JsonValue::Array(vec![JsonValue::String(filename.to_string()), data]))
        }
        JsonValue::Object(map) => {
            let filename = map.get("filename").or_else(|| map.get("name"))?.as_str()?;
            let data = map.get("data").and_then(normalize_attachment_data)?;
            Some(JsonValue::Array(vec![JsonValue::String(filename.to_string()), data]))
        }
        _ => None,
    }
}

fn normalize_attachment_data(value: &JsonValue) -> Option<JsonValue> {
    let bytes =
        match value {
            JsonValue::Array(items) => {
                let mut normalized = Vec::with_capacity(items.len());
                for item in items {
                    let byte =
                        item.as_u64()
                            .and_then(|value| {
                                if value <= u8::MAX as u64 {
                                    Some(value as u8)
                                } else {
                                    None
                                }
                            })
                            .or_else(|| item.as_i64().and_then(|value| u8::try_from(value).ok()));
                    let byte = byte?;
                    normalized.push(byte);
                }
                normalized
            }
            JsonValue::String(text) => decode_attachment_text_data(text)?,
            _ => return None,
        };

    Some(JsonValue::Array(
        bytes.into_iter().map(|byte| JsonValue::Number(serde_json::Number::from(byte))).collect(),
    ))
}

fn decode_hex_attachment_data(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 || !text.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < text.len() {
        bytes.push(u8::from_str_radix(&text[index..index + 2], 16).ok()?);
        index += 2;
    }
    Some(bytes)
}

fn decode_attachment_text_data(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    if let Some(payload) = text.strip_prefix("hex:").or_else(|| text.strip_prefix("HEX:")) {
        return decode_hex_attachment_data(payload.trim());
    }

    if let Some(payload) = text.strip_prefix("base64:").or_else(|| text.strip_prefix("BASE64:")) {
        return BASE64_STANDARD.decode(payload.trim()).ok();
    }

    let hex = decode_hex_attachment_data(text);
    let base64 = BASE64_STANDARD.decode(text).ok();
    match (hex, base64) {
        (Some(bytes), None) => Some(bytes),
        (None, Some(bytes)) => Some(bytes),
        (Some(_), Some(_)) => None,
        (None, None) => None,
    }
}

fn json_to_rmpv_lossless(value: &JsonValue) -> Result<Value, LxmfError> {
    match value {
        JsonValue::Null => Ok(Value::Nil),
        JsonValue::Bool(value) => Ok(Value::Boolean(*value)),
        JsonValue::Number(value) => {
            if let Some(int) = value.as_i64() {
                Ok(Value::Integer(int.into()))
            } else if let Some(int) = value.as_u64() {
                Ok(Value::Integer(int.into()))
            } else if let Some(float) = value.as_f64() {
                Ok(Value::F64(float))
            } else {
                Err(LxmfError::Encode("invalid number".to_string()))
            }
        }
        JsonValue::String(value) => Ok(Value::String(value.as_str().into())),
        JsonValue::Array(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                out.push(json_to_rmpv_lossless(value)?);
            }
            Ok(Value::Array(out))
        }
        JsonValue::Object(map) => {
            let mut out = Vec::with_capacity(map.len());
            for (key, value) in map {
                out.push((json_key_to_rmpv(key), json_to_rmpv_lossless(value)?));
            }
            Ok(Value::Map(out))
        }
    }
}

fn json_key_to_rmpv(key: &str) -> Value {
    if is_canonical_signed_integer_key(key) {
        if let Ok(value) = key.parse::<i64>() {
            return Value::Integer(value.into());
        }
    }
    if is_canonical_unsigned_integer_key(key) {
        if let Ok(value) = key.parse::<u64>() {
            return Value::Integer(value.into());
        }
    }
    Value::String(key.into())
}

fn is_canonical_unsigned_integer_key(key: &str) -> bool {
    if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    key == "0" || !key.starts_with('0')
}

fn is_canonical_signed_integer_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix('-') else {
        return false;
    };
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    rest != "0" && !rest.starts_with('0')
}

pub fn rmpv_to_json(value: &Value) -> Option<JsonValue> {
    match value {
        Value::Nil => Some(JsonValue::Null),
        Value::Boolean(v) => Some(JsonValue::Bool(*v)),
        Value::Integer(v) => v
            .as_i64()
            .map(|i| JsonValue::Number(i.into()))
            .or_else(|| v.as_u64().map(|u| JsonValue::Number(u.into()))),
        Value::F32(v) => serde_json::Number::from_f64(f64::from(*v)).map(JsonValue::Number),
        Value::F64(v) => serde_json::Number::from_f64(*v).map(JsonValue::Number),
        Value::String(s) => s.as_str().map(|v| JsonValue::String(v.to_string())),
        Value::Binary(bytes) => {
            Some(JsonValue::Array(bytes.iter().map(|b| JsonValue::Number((*b).into())).collect()))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(rmpv_to_json(item)?);
            }
            Some(JsonValue::Array(out))
        }
        Value::Map(entries) => {
            let mut object = serde_json::Map::new();
            for (key, value) in entries {
                let key_str = match key {
                    Value::String(text) => text.as_str().map(|v| v.to_string()),
                    Value::Integer(int) => int
                        .as_i64()
                        .map(|v| v.to_string())
                        .or_else(|| int.as_u64().map(|v| v.to_string())),
                    other => Some(format!("{:?}", other)),
                }?;
                object.insert(key_str, rmpv_to_json(value)?);
            }
            Some(JsonValue::Object(object))
        }
        _ => None,
    }
}
