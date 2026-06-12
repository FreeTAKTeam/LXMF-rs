fn enrich_app_extension_fields(object: &mut JsonMap<String, JsonValue>) {
    let Some(app_extensions) = object.get("16").and_then(JsonValue::as_object).cloned() else {
        return;
    };

    if let Some(reaction_to) = app_extensions.get("reaction_to").and_then(JsonValue::as_str) {
        object.insert("is_reaction".to_string(), JsonValue::Bool(true));
        object.insert("reaction_to".to_string(), JsonValue::String(reaction_to.to_string()));
        if let Some(emoji) = app_extensions.get("emoji").and_then(JsonValue::as_str) {
            object.insert("reaction_emoji".to_string(), JsonValue::String(emoji.to_string()));
        }
        if let Some(sender) = app_extensions.get("sender").and_then(JsonValue::as_str) {
            object.insert("reaction_sender".to_string(), JsonValue::String(sender.to_string()));
        }
    }

    if let Some(reply_to) = app_extensions.get("reply_to").and_then(JsonValue::as_str) {
        object.insert("reply_to".to_string(), JsonValue::String(reply_to.to_string()));
    }
}

fn decode_binary_bytes(value: &Value) -> Option<&[u8]> {
    match value {
        Value::Binary(bytes) => Some(bytes.as_slice()),
        _ => None,
    }
}

fn decode_i32_be(value: &Value) -> Option<i32> {
    let bytes = decode_binary_bytes(value)?;
    if bytes.len() != 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(bytes);
    Some(i32::from_be_bytes(raw))
}

fn decode_u32_be(value: &Value) -> Option<u32> {
    let bytes = decode_binary_bytes(value)?;
    if bytes.len() != 4 {
        return None;
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(bytes);
    Some(u32::from_be_bytes(raw))
}

fn decode_u16_be(value: &Value) -> Option<u16> {
    let bytes = decode_binary_bytes(value)?;
    if bytes.len() != 2 {
        return None;
    }
    let mut raw = [0u8; 2];
    raw.copy_from_slice(bytes);
    Some(u16::from_be_bytes(raw))
}
