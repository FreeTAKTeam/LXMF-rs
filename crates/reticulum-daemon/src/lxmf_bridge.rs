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
    if let Ok(value) = key.parse::<i64>() {
        return Value::Integer(value.into());
    }
    if let Ok(value) = key.parse::<u64>() {
        return Value::Integer(value.into());
    }
    Value::String(key.into())
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
                if key_str == "2" {
                    if let Value::Binary(bytes) = value {
                        if let Some(decoded) = decode_sideband_location_telemetry(bytes) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                    if let Value::String(text) = value {
                        if let Some(decoded) = decode_sideband_location_telemetry(text.as_bytes()) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                }
                if key_str == "3" {
                    if let Value::Binary(bytes) = value {
                        if let Some(decoded) = decode_telemetry_stream(bytes) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                    if let Value::String(text) = value {
                        if let Some(decoded) = decode_telemetry_stream(text.as_bytes()) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                }
                if key_str == "112" {
                    if let Value::String(text) = value {
                        if let Some(decoded) = text.as_str().and_then(decode_columba_meta) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    } else if let Value::Binary(bytes) = value {
                        if let Some(decoded) = decode_columba_meta_bytes(bytes) {
                            object.insert(key_str, decoded);
                            continue;
                        }
                    }
                }
                object.insert(key_str, rmpv_to_json(value)?);
            }
            Some(JsonValue::Object(object))
        }
        _ => None,
    }
}

fn decode_sideband_location_telemetry(packed: &[u8]) -> Option<JsonValue> {
    let mut cursor = std::io::Cursor::new(packed);
    let decoded = rmpv::decode::read_value(&mut cursor).ok()?;
    let rmpv::Value::Map(map) = decoded else {
        return None;
    };
    let location = map
        .iter()
        .find(|(key, _)| key.as_i64() == Some(0x02) || key.as_u64() == Some(0x02))
        .map(|(_, value)| value)?;
    let rmpv::Value::Array(items) = location else {
        return None;
    };
    if items.len() < 7 {
        return None;
    }

    let lat = decode_i32_be(items.first()?)? as f64 / 1e6;
    let lon = decode_i32_be(items.get(1)?)? as f64 / 1e6;
    let alt = decode_i32_be(items.get(2)?)? as f64 / 1e2;
    let speed = decode_u32_be(items.get(3)?)? as f64 / 1e2;
    let bearing = decode_i32_be(items.get(4)?)? as f64 / 1e2;
    let accuracy = decode_u16_be(items.get(5)?)? as f64 / 1e2;
    let updated = items.get(6).and_then(|value| {
        value.as_i64().or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
    });

    let mut out = serde_json::Map::new();
    out.insert("lat".to_string(), JsonValue::from(lat));
    out.insert("lon".to_string(), JsonValue::from(lon));
    out.insert("alt".to_string(), JsonValue::from(alt));
    out.insert("speed".to_string(), JsonValue::from(speed));
    out.insert("bearing".to_string(), JsonValue::from(bearing));
    out.insert("accuracy".to_string(), JsonValue::from(accuracy));
    if let Some(updated) = updated {
        out.insert("updated".to_string(), JsonValue::from(updated));
    }
    Some(JsonValue::Object(out))
}

fn decode_telemetry_stream(packed: &[u8]) -> Option<JsonValue> {
    let mut cursor = std::io::Cursor::new(packed);
    let decoded = rmpv::decode::read_value(&mut cursor).ok()?;
    rmpv_to_json(&decoded)
}

fn decode_i32_be(value: &Value) -> Option<i32> {
    let value = decode_binary_bytes(value)?;
    if value.len() != 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(value);
    Some(i32::from_be_bytes(raw))
}

fn decode_u32_be(value: &Value) -> Option<u32> {
    let value = decode_binary_bytes(value)?;
    if value.len() != 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(value);
    Some(u32::from_be_bytes(raw))
}

fn decode_u16_be(value: &Value) -> Option<u16> {
    let value = decode_binary_bytes(value)?;
    if value.len() != 2 {
        return None;
    }
    let mut raw = [0u8; 2];
    raw.copy_from_slice(value);
    Some(u16::from_be_bytes(raw))
}

fn decode_binary_bytes(value: &Value) -> Option<&[u8]> {
    match value {
        Value::Binary(bytes) => Some(bytes.as_slice()),
        _ => None,
    }
}

fn decode_columba_meta(text: &str) -> Option<JsonValue> {
    if let Ok(json) = serde_json::from_str::<JsonValue>(text) {
        Some(json)
    } else {
        Some(JsonValue::String(text.to_string()))
    }
}

fn decode_columba_meta_bytes(bytes: &[u8]) -> Option<JsonValue> {
    let text = std::str::from_utf8(bytes).ok();
    if let Some(text) = text {
        if let Ok(json) = serde_json::from_str::<JsonValue>(text) {
            return Some(json);
        }
    }
    let mut cursor = std::io::Cursor::new(bytes);
    if let Ok(decoded) = rmpv::decode::read_value(&mut cursor) {
        if usize::try_from(cursor.position()).ok() == Some(bytes.len())
            && let Some(decoded) = rmpv_to_json(&decoded)
        {
            return Some(decoded);
        }
    }
    text.map(|value| JsonValue::String(value.to_string()))
        .or_else(|| rmpv_to_json(&Value::Binary(bytes.to_vec())))
}
