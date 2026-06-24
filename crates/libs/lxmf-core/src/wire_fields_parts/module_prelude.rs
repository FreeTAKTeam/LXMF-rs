use crate::LxmfError;

use alloc::format;

use alloc::string::String;

use alloc::string::ToString;

use alloc::vec;

use alloc::vec::Vec;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use base64::Engine as _;

use rmpv::Value;

use serde_json::{Map as JsonMap, Value as JsonValue};

const FIELD_ATTACHMENTS_WIRE_KEY: &str = "5";

const FIELD_ATTACHMENTS_PUBLIC_KEY: &str = "attachments";

const FIELD_ATTACHMENTS_LEGACY_FILES_KEY: &str = "files";

type ClientFieldDecoder = fn(&Value, RmpvToJsonOptions) -> Result<JsonValue, LxmfError>;

const CLIENT_FIELD_DECODERS: [(&str, ClientFieldDecoder); 3] = [
    ("2", decode_client_sideband_location),
    ("3", decode_client_telemetry_stream),
    ("112", decode_client_columba_meta),
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RmpvToJsonOptions {
    pub enrich_app_extensions: bool,
}

pub fn contains_attachment_aliases(fields: Option<&JsonValue>) -> bool {
    let Some(JsonValue::Object(map)) = fields else {
        return false;
    };

    [FIELD_ATTACHMENTS_WIRE_KEY, FIELD_ATTACHMENTS_PUBLIC_KEY, FIELD_ATTACHMENTS_LEGACY_FILES_KEY]
        .into_iter()
        .any(|key| map.contains_key(key))
}

pub fn normalize_attachment_fields_for_wire(
    fields: &mut JsonMap<String, JsonValue>,
) -> Result<(), LxmfError> {
    let canonical = fields.contains_key(FIELD_ATTACHMENTS_PUBLIC_KEY);
    let legacy = fields.contains_key(FIELD_ATTACHMENTS_LEGACY_FILES_KEY);
    let raw_wire = fields.contains_key(FIELD_ATTACHMENTS_WIRE_KEY);
    let alias_count = usize::from(canonical) + usize::from(legacy) + usize::from(raw_wire);

    if alias_count > 1 {
        return Err(LxmfError::Encode(format!(
            "attachment aliases '{}', '{}', and '{}' cannot be mixed",
            FIELD_ATTACHMENTS_PUBLIC_KEY,
            FIELD_ATTACHMENTS_LEGACY_FILES_KEY,
            FIELD_ATTACHMENTS_WIRE_KEY,
        )));
    }

    if raw_wire {
        return Ok(());
    }

    let source_key = if canonical {
        FIELD_ATTACHMENTS_PUBLIC_KEY
    } else if legacy {
        FIELD_ATTACHMENTS_LEGACY_FILES_KEY
    } else {
        return Ok(());
    };

    let Some(raw_entries) = fields.remove(source_key) else {
        return Ok(());
    };

    let entries = raw_entries.as_array().ok_or_else(|| {
        LxmfError::Encode(format!("field '{}' must be an array of attachment objects", source_key))
    })?;
    if entries.is_empty() {
        return Ok(());
    }

    let normalized = normalize_file_attachments(entries)?;
    fields.insert(FIELD_ATTACHMENTS_WIRE_KEY.to_string(), JsonValue::Array(normalized));
    Ok(())
}

pub fn json_to_rmpv(value: &JsonValue) -> Result<Value, LxmfError> {
    let mut normalized = value.clone();
    if let JsonValue::Object(map) = &mut normalized {
        normalize_attachment_fields_for_wire(map)?;
    }
    json_to_rmpv_lossless(&normalized)
}

pub fn rmpv_to_json(value: &Value) -> Result<JsonValue, LxmfError> {
    rmpv_to_json_with_options(value, RmpvToJsonOptions::default())
}

pub fn rmpv_to_json_with_options(
    value: &Value,
    options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    rmpv_to_json_with_options_inner(value, options)
}

fn normalize_file_attachments(entries: &[JsonValue]) -> Result<Vec<JsonValue>, LxmfError> {
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        normalized.push(normalize_file_attachment_entry(entry)?);
    }
    Ok(normalized)
}

fn normalize_file_attachment_entry(entry: &JsonValue) -> Result<JsonValue, LxmfError> {
    match entry {
        JsonValue::Object(map) => {
            let filename = map.get("name").and_then(JsonValue::as_str).ok_or_else(|| {
                LxmfError::Encode("attachment entry must include string field 'name'".to_string())
            })?;
            let data = map
                .get("data")
                .ok_or_else(|| {
                    LxmfError::Encode("attachment entry must include field 'data'".to_string())
                })
                .and_then(normalize_attachment_data)?;
            Ok(JsonValue::Array(vec![JsonValue::String(filename.to_string()), data]))
        }
        _ => Err(LxmfError::Encode("attachments must be objects with canonical shape".to_string())),
    }
}

fn normalize_attachment_data(value: &JsonValue) -> Result<JsonValue, LxmfError> {
    let bytes = match value {
        JsonValue::Array(items) => {
            let mut normalized = Vec::with_capacity(items.len());
            for item in items {
                let byte = item
                    .as_u64()
                    .and_then(
                        |value| {
                            if value <= u8::MAX as u64 {
                                Some(value as u8)
                            } else {
                                None
                            }
                        },
                    )
                    .or_else(|| item.as_i64().and_then(|value| u8::try_from(value).ok()));
                let byte = byte.ok_or_else(|| {
                    LxmfError::Encode(
                        "attachment data array must contain bytes between 0 and 255".to_string(),
                    )
                })?;
                normalized.push(byte);
            }
            normalized
        }
        JsonValue::String(text) => decode_attachment_text_data(text)?,
        _ => {
            return Err(LxmfError::Encode(
                "attachment data must be an array of bytes or prefixed text data".to_string(),
            ))
        }
    };

    Ok(JsonValue::Array(
        bytes.into_iter().map(|byte| JsonValue::Number(serde_json::Number::from(byte))).collect(),
    ))
}

fn decode_hex_attachment_data(text: &str) -> Result<Vec<u8>, LxmfError> {
    if text.len() % 2 != 0 || !text.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(LxmfError::Encode(
            "attachment hex data has odd length or non-hex characters".to_string(),
        ));
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < text.len() {
        bytes.push(
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|err| LxmfError::Encode(format!("invalid hex attachment data: {err}")))?,
        );
        index += 2;
    }
    Ok(bytes)
}

fn decode_attachment_text_data(text: &str) -> Result<Vec<u8>, LxmfError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(LxmfError::Encode("attachment data string cannot be empty".to_string()));
    }

    if let Some(payload) = text.strip_prefix("hex:").or_else(|| text.strip_prefix("HEX:")) {
        return decode_hex_attachment_data(payload.trim());
    }

    if let Some(payload) = text.strip_prefix("base64:").or_else(|| text.strip_prefix("BASE64:")) {
        return BASE64_STANDARD.decode(payload.trim()).map_err(|err| {
            LxmfError::Encode(format!("invalid base64 attachment data after base64: prefix: {err}"))
        });
    }

    Err(LxmfError::Encode(
        "attachment text data must use explicit 'hex:' or 'base64:' prefix".to_string(),
    ))
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
    if let Some(value) = parse_canonical_numeric_key(key) {
        return Value::Integer(value.into());
    }
    Value::String(key.into())
}

fn parse_canonical_numeric_key(key: &str) -> Option<i64> {
    if key.is_empty() {
        return None;
    }

    if let Some(digits) = key.strip_prefix('-') {
        if digits.is_empty() {
            return None;
        }
        if digits.len() > 1 && digits.starts_with('0') {
            return None;
        }
        return key.parse::<i64>().ok();
    }

    if key.len() > 1 && key.starts_with('0') {
        return None;
    }

    key.parse::<i64>().ok()
}

fn rmpv_to_json_with_options_inner(
    value: &Value,
    options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    match value {
        Value::Nil => Ok(JsonValue::Null),
        Value::Boolean(v) => Ok(JsonValue::Bool(*v)),
        Value::Integer(v) => v
            .as_i64()
            .map(|i| JsonValue::Number(i.into()))
            .or_else(|| v.as_u64().map(|u| JsonValue::Number(u.into())))
            .ok_or_else(|| LxmfError::Decode("msgpack integer out of JSON range".to_string())),
        Value::F32(v) => serde_json::Number::from_f64(f64::from(*v))
            .map(JsonValue::Number)
            .ok_or_else(|| LxmfError::Decode("non-finite f32 cannot be represented as JSON".to_string())),
        Value::F64(v) => serde_json::Number::from_f64(*v)
            .map(JsonValue::Number)
            .ok_or_else(|| LxmfError::Decode("non-finite f64 cannot be represented as JSON".to_string())),
        Value::String(s) => s
            .as_str()
            .map(|v| JsonValue::String(v.to_string()))
            .ok_or_else(|| LxmfError::Decode("msgpack string is not valid UTF-8".to_string())),
        Value::Binary(bytes) => {
            Ok(JsonValue::Array(bytes.iter().map(|b| JsonValue::Number((*b).into())).collect()))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(rmpv_to_json_with_options_inner(item, options)?);
            }
            Ok(JsonValue::Array(out))
        }
        Value::Map(entries) => {
            let mut object = JsonMap::new();
            for (key, value) in entries {
                let key_str = match key {
                    Value::String(text) => text.as_str().map(|v| v.to_string()).ok_or_else(|| {
                        LxmfError::Decode("msgpack map key is not valid UTF-8".to_string())
                    })?,
                    Value::Integer(int) => int
                        .as_i64()
                        .map(|v| v.to_string())
                        .or_else(|| int.as_u64().map(|v| v.to_string()))
                        .ok_or_else(|| {
                            LxmfError::Decode("msgpack map key integer out of range".to_string())
                        })?,
                    other => format!("{other:?}"),
                };

                if let Some(Ok(decoded)) =
                    decode_client_specific_field(key_str.as_str(), value, options)
                {
                    object.insert(key_str, decoded);
                    continue;
                }
                // The key matched a client-specific decoder but the value is not in that
                // format (e.g. a generic value that merely shares the key, like `{2: 1}`).
                // Fall back to the generic representation rather than rejecting the whole
                // message — the value is preserved, just not specially decoded.
                object.insert(key_str, rmpv_to_json_with_options_inner(value, options)?);
            }

            if options.enrich_app_extensions {
                enrich_app_extension_fields(&mut object);
            }
            Ok(JsonValue::Object(object))
        }
        _ => Err(LxmfError::Decode("unsupported msgpack value for JSON conversion".to_string())),
    }
}

fn decode_client_specific_field(
    field_key: &str,
    value: &Value,
    options: RmpvToJsonOptions,
) -> Option<Result<JsonValue, LxmfError>> {
    CLIENT_FIELD_DECODERS
        .iter()
        .find_map(|(target_key, decoder)| {
            (*target_key == field_key).then(|| decoder(value, options))
        })
}

fn decode_client_sideband_location(
    value: &Value,
    _options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    match value {
        Value::Binary(bytes) => decode_sideband_location_telemetry(bytes),
        Value::String(text) => decode_sideband_location_telemetry(text.as_bytes()),
        _ => Err(LxmfError::Decode("sideband location field is not binary or string".to_string())),
    }
}

fn decode_client_telemetry_stream(
    value: &Value,
    options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    match value {
        Value::Binary(bytes) => decode_telemetry_stream(bytes, options),
        Value::String(text) => decode_telemetry_stream(text.as_bytes(), options),
        _ => Err(LxmfError::Decode("telemetry stream field is not binary or string".to_string())),
    }
}

fn decode_client_columba_meta(
    value: &Value,
    options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    match value {
        Value::String(text) => {
            let text = text.as_str().ok_or_else(|| {
                LxmfError::Decode("columba meta string is not valid UTF-8".to_string())
            })?;
            Ok(decode_columba_meta_text(text))
        }
        Value::Binary(bytes) => decode_columba_meta_bytes(bytes, options),
        _ => Err(LxmfError::Decode("columba meta field is not string or binary".to_string())),
    }
}

fn sideband_location_i32(items: &[Value], idx: usize) -> Result<i32, LxmfError> {
    let value = items.get(idx).ok_or_else(|| {
        LxmfError::Decode("sideband telemetry location element missing".to_string())
    })?;
    decode_i32_be(value)
        .ok_or_else(|| LxmfError::Decode("sideband telemetry location element is not i32".to_string()))
}

fn sideband_location_u32(items: &[Value], idx: usize) -> Result<u32, LxmfError> {
    let value = items.get(idx).ok_or_else(|| {
        LxmfError::Decode("sideband telemetry location element missing".to_string())
    })?;
    decode_u32_be(value)
        .ok_or_else(|| LxmfError::Decode("sideband telemetry location element is not u32".to_string()))
}

fn sideband_location_u16(items: &[Value], idx: usize) -> Result<u16, LxmfError> {
    let value = items.get(idx).ok_or_else(|| {
        LxmfError::Decode("sideband telemetry location element missing".to_string())
    })?;
    decode_u16_be(value)
        .ok_or_else(|| LxmfError::Decode("sideband telemetry location element is not u16".to_string()))
}

fn decode_sideband_location_telemetry(packed: &[u8]) -> Result<JsonValue, LxmfError> {
    let decoded = decode_msgpack_value_from_bytes(packed)?;
    let Value::Map(map) = decoded else {
        return Err(LxmfError::Decode("sideband telemetry payload is not a map".to_string()));
    };
    let location = map
        .iter()
        .find(|(key, _)| key.as_i64() == Some(0x02) || key.as_u64() == Some(0x02))
        .map(|(_, value)| value)
        .ok_or_else(|| LxmfError::Decode("sideband telemetry missing location field".to_string()))?;
    let Value::Array(items) = location else {
        return Err(LxmfError::Decode("sideband telemetry location is not an array".to_string()));
    };
    if items.len() < 7 {
        return Err(LxmfError::Decode(
            "sideband telemetry location has too few elements".to_string(),
        ));
    }

    let lat = sideband_location_i32(items, 0)? as f64 / 1e6;
    let lon = sideband_location_i32(items, 1)? as f64 / 1e6;
    let alt = sideband_location_i32(items, 2)? as f64 / 1e2;
    let speed = sideband_location_u32(items, 3)? as f64 / 1e2;
    let bearing = sideband_location_i32(items, 4)? as f64 / 1e2;
    let accuracy = sideband_location_u16(items, 5)? as f64 / 1e2;
    let updated = items.get(6).and_then(|value| {
        value.as_i64().or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
    });

    let mut out = JsonMap::new();
    out.insert("lat".to_string(), JsonValue::from(lat));
    out.insert("lon".to_string(), JsonValue::from(lon));
    out.insert("alt".to_string(), JsonValue::from(alt));
    out.insert("speed".to_string(), JsonValue::from(speed));
    out.insert("bearing".to_string(), JsonValue::from(bearing));
    out.insert("accuracy".to_string(), JsonValue::from(accuracy));
    if let Some(updated) = updated {
        out.insert("updated".to_string(), JsonValue::from(updated));
    }
    Ok(JsonValue::Object(out))
}

fn decode_telemetry_stream(
    packed: &[u8],
    options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    let decoded = decode_msgpack_value_from_bytes(packed)?;
    rmpv_to_json_with_options_inner(&decoded, options)
}

fn decode_columba_meta_text(text: &str) -> JsonValue {
    // Best-effort preservation: parse as JSON, otherwise keep the raw string.
    // Never fails — every input yields a value, so this is plain `T`, not `Option`.
    serde_json::from_str::<JsonValue>(text).unwrap_or_else(|_| JsonValue::String(text.to_string()))
}

fn decode_columba_meta_bytes(
    bytes: &[u8],
    options: RmpvToJsonOptions,
) -> Result<JsonValue, LxmfError> {
    // Columba metadata is preserved best-effort: try JSON, then exact msgpack,
    // then fall back to the raw string / binary representation (always succeeds).
    if let Ok(text) = core::str::from_utf8(bytes) {
        if let Ok(json) = serde_json::from_str::<JsonValue>(text) {
            return Ok(json);
        }
    }

    if let Ok(decoded) = decode_msgpack_value_from_bytes_exact(bytes) {
        if let Ok(decoded) = rmpv_to_json_with_options_inner(&decoded, options) {
            return Ok(decoded);
        }
    }

    if let Ok(text) = core::str::from_utf8(bytes) {
        return Ok(JsonValue::String(text.to_string()));
    }

    rmpv_to_json_with_options_inner(&Value::Binary(bytes.to_vec()), options)
}

#[cfg(feature = "std")]
fn decode_msgpack_value_from_bytes(bytes: &[u8]) -> Result<Value, LxmfError> {
    let mut cursor = std::io::Cursor::new(bytes);
    rmpv::decode::read_value(&mut cursor)
        .map_err(|err| LxmfError::Decode(format!("msgpack value decode failed: {err}")))
}

#[cfg(not(feature = "std"))]
fn decode_msgpack_value_from_bytes(bytes: &[u8]) -> Result<Value, LxmfError> {
    rmp_serde::from_slice(bytes)
        .map_err(|err| LxmfError::Decode(format!("msgpack value decode failed: {err}")))
}

#[cfg(feature = "std")]
fn decode_msgpack_value_from_bytes_exact(bytes: &[u8]) -> Result<Value, LxmfError> {
    let mut cursor = std::io::Cursor::new(bytes);
    let decoded = rmpv::decode::read_value(&mut cursor)
        .map_err(|err| LxmfError::Decode(format!("msgpack value decode failed: {err}")))?;
    if usize::try_from(cursor.position()).ok() == Some(bytes.len()) {
        Ok(decoded)
    } else {
        Err(LxmfError::Decode("msgpack value had trailing bytes".to_string()))
    }
}

#[cfg(not(feature = "std"))]
fn decode_msgpack_value_from_bytes_exact(bytes: &[u8]) -> Result<Value, LxmfError> {
    rmp_serde::from_slice(bytes)
        .map_err(|err| LxmfError::Decode(format!("msgpack value decode failed: {err}")))
}
