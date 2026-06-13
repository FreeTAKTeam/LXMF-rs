fn pn_metadata_to_json(value: &MsgPackValue) -> Option<JsonValue> {
    let MsgPackValue::Map(entries) = value else {
        return None;
    };
    let mut metadata = JsonMap::new();
    for (key, value) in entries {
        let Some(key) = pn_metadata_key_to_string(key) else {
            continue;
        };
        let Some(value) = pn_metadata_value_to_json(value) else {
            continue;
        };
        metadata.insert(key, value);
    }
    Some(JsonValue::Object(metadata))
}

fn pn_metadata_key_to_string(key: &MsgPackValue) -> Option<String> {
    if is_pn_name_metadata_key(key) {
        return Some("name".to_string());
    }
    match key {
        MsgPackValue::Integer(value) => value.as_u64().map(|value| value.to_string()),
        MsgPackValue::String(text) => text.as_str().map(str::to_string),
        MsgPackValue::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

fn pn_metadata_value_to_json(value: &MsgPackValue) -> Option<JsonValue> {
    match value {
        MsgPackValue::Nil => Some(JsonValue::Null),
        MsgPackValue::Boolean(value) => Some(json!(value)),
        MsgPackValue::Integer(value) => value
            .as_i64()
            .map(JsonValue::from)
            .or_else(|| value.as_u64().map(JsonValue::from)),
        MsgPackValue::F32(value) => Some(json!(f64::from(*value))),
        MsgPackValue::F64(value) => Some(json!(value)),
        MsgPackValue::String(text) => text.as_str().map(JsonValue::from),
        MsgPackValue::Binary(bytes) => String::from_utf8(bytes.clone()).ok().map(JsonValue::from),
        _ => None,
    }
}

fn parse_pn_metadata_name(value: &MsgPackValue) -> Option<String> {
    let MsgPackValue::Map(entries) = value else {
        return None;
    };

    for (key, value) in entries {
        if is_pn_name_metadata_key(key) {
            return msgpack_value_to_clean_name(value);
        }
    }
    None
}

fn is_pn_name_metadata_key(key: &MsgPackValue) -> bool {
    const PN_META_NAME: u64 = 1;
    match key {
        MsgPackValue::Integer(value) => value.as_u64() == Some(PN_META_NAME),
        MsgPackValue::String(text) => text
            .as_str()
            .is_some_and(|value| matches!(value.trim(), "name" | "n" | "display_name")),
        MsgPackValue::Binary(bytes) => std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|value| matches!(value.trim(), "name" | "n" | "display_name")),
        _ => false,
    }
}

fn msgpack_value_to_clean_name(value: &MsgPackValue) -> Option<String> {
    let name = match value {
        MsgPackValue::Binary(bytes) => String::from_utf8(bytes.clone()).ok()?,
        MsgPackValue::String(text) => text.as_str()?.to_string(),
        _ => return None,
    };
    let name = clean_optional_text(Some(name))?;
    if name.chars().any(char::is_control) {
        return None;
    }
    first_n_chars(name.as_str(), 64).or(Some(name))
}

fn parse_delivery_stamp_cost_from_app_data_hex(app_data_hex: Option<&str>) -> Option<u32> {
    let raw_hex = app_data_hex.map(str::trim).filter(|value| !value.is_empty())?;
    let app_data = hex::decode(raw_hex).ok()?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data).ok()?;
    let entries = value.as_array()?;
    entries.get(1).and_then(parse_fuzzy_u32).filter(|cost| (1..255).contains(cost))
}

fn is_lxmf_delivery_aspect(aspect: Option<&str>) -> bool {
    matches!(
        aspect.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("lxmf.delivery" | "delivery")
    )
}

fn inbound_ticket_from_record(record: &MessageRecord) -> Option<(i64, String)> {
    let fields = record.fields.as_ref()?.as_object()?;
    let lxmf = fields.get("_lxmf").and_then(JsonValue::as_object);
    if lxmf.and_then(|value| value.get("signature_valid")).and_then(JsonValue::as_bool)
        != Some(true)
    {
        return None;
    }

    let ticket_entry = fields.get("12")?.as_array()?;
    let expires_at = ticket_entry.first().and_then(json_value_to_i64)?;
    let ticket = ticket_entry.get(1).and_then(json_ticket_to_hex)?;
    (ticket.len() == 32).then_some((expires_at, ticket))
}

fn json_value_to_i64(value: &JsonValue) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| {
            let value = value.as_f64()?;
            if !value.is_finite() {
                return None;
            }
            let rounded = value.ceil();
            if rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
                return None;
            }
            Some(rounded as i64)
        })
}

fn json_ticket_to_hex(value: &JsonValue) -> Option<String> {
    let bytes = value
        .as_array()?
        .iter()
        .map(|item| item.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect::<Option<Vec<_>>>()?;
    (bytes.len() == 16).then(|| hex::encode(bytes))
}

fn extract_capabilities_from_msgpack(value: &MsgPackValue) -> Option<Vec<String>> {
    if let MsgPackValue::Array(entries) = value {
        return Some(normalize_capabilities(
            entries.iter().filter_map(capability_value_to_string).collect(),
        ));
    }

    let MsgPackValue::Map(entries) = value else {
        return None;
    };
    entries.iter().find_map(|(key, value)| {
        if is_capability_key(key) {
            return extract_capabilities_from_msgpack(value);
        }
        None
    })
}

fn is_capability_key(key: &MsgPackValue) -> bool {
    msgpack_key_to_string(key).is_some_and(|name| matches!(name.as_str(), "caps" | "capabilities"))
}

fn capability_value_to_string(value: &MsgPackValue) -> Option<String> {
    match value {
        MsgPackValue::String(text) => text.as_str().map(str::to_string),
        MsgPackValue::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

fn parse_capabilities_from_utf8_app_data(app_data: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(app_data) else {
        return Vec::new();
    };
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    if let Ok(value) = serde_json::from_str::<JsonValue>(text) {
        let capabilities = extract_capabilities_from_json_value(&value);
        if !capabilities.is_empty() {
            return capabilities;
        }
    }

    parse_capabilities_from_tagged_text(text)
}

fn extract_capabilities_from_json_value(value: &JsonValue) -> Vec<String> {
    match value {
        JsonValue::Array(values) => normalize_capabilities(
            values.iter().filter_map(json_capability_value_to_string).collect(),
        ),
        JsonValue::Object(map) => {
            for key in ["capabilities", "caps"] {
                if let Some(value) = map.get(key) {
                    let capabilities = extract_capabilities_from_json_value(value);
                    if !capabilities.is_empty() {
                        return capabilities;
                    }
                }
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn json_capability_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(value) => Some(value.to_string()),
        _ => None,
    }
}

fn parse_capabilities_from_tagged_text(text: &str) -> Vec<String> {
    let lowered = text.to_ascii_lowercase();
    for marker in ["capabilities=", "caps=", "capabilities:", "caps:"] {
        if let Some(index) = lowered.find(marker) {
            let tail = &text[index + marker.len()..];
            let candidate = tail
                .split([';', '\n', '\r'])
                .next()
                .unwrap_or_default()
                .trim()
                .trim_matches(|ch| matches!(ch, '[' | ']' | '"' | '\''));
            if !candidate.is_empty() {
                let capabilities = candidate
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let capabilities = normalize_capabilities(capabilities);
                if !capabilities.is_empty() {
                    return capabilities;
                }
            }
        }
    }
    Vec::new()
}

fn msgpack_key_to_string(key: &MsgPackValue) -> Option<String> {
    match key {
        MsgPackValue::String(key) => key.as_str().map(|key| key.trim().to_ascii_lowercase()),
        MsgPackValue::Binary(key) => {
            String::from_utf8(key.clone()).ok().map(|key| key.trim().to_ascii_lowercase())
        }
        _ => None,
    }
}

fn encode_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

const LEGACY_EVENT_QUEUE_CAPACITY: usize = 32;

const SDK_EVENT_LOG_CAPACITY: usize = 1024;

const SDK_STREAM_ID: &str = "sdk-events";
