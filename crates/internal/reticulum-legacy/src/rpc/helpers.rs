fn merge_fields_with_options(
    fields: Option<JsonValue>,
    method: Option<String>,
    stamp_cost: Option<u32>,
    include_ticket: Option<bool>,
) -> Option<JsonValue> {
    let has_options = method.is_some() || stamp_cost.is_some() || include_ticket.is_some();
    if !has_options {
        return fields;
    }

    let mut root = match fields {
        Some(JsonValue::Object(map)) => map,
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("_fields_raw".into(), other);
            map
        }
        None => JsonMap::new(),
    };

    let mut lxmf = JsonMap::new();
    if let Some(value) = method {
        lxmf.insert("method".into(), JsonValue::String(value));
    }
    if let Some(value) = stamp_cost {
        lxmf.insert("stamp_cost".into(), json!(value));
    }
    if let Some(value) = include_ticket {
        lxmf.insert("include_ticket".into(), json!(value));
    }

    root.insert("_lxmf".into(), JsonValue::Object(lxmf));
    Some(JsonValue::Object(root))
}

fn now_i64() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

fn first_n_chars(input: &str, n: usize) -> Option<String> {
    if n == 0 {
        return Some(String::new());
    }
    let end = input.char_indices().nth(n - 1).map(|(idx, ch)| idx + ch.len_utf8())?;
    Some(input[..end].to_string())
}

fn clean_optional_text(value: Option<String>) -> Option<String> {
    value.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn normalize_capabilities(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in values {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        out.push(normalized);
    }
    out
}

fn parse_capabilities_from_app_data_hex(app_data_hex: Option<&str>) -> Vec<String> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let Ok(app_data) = hex::decode(raw_hex) else {
        return Vec::new();
    };
    if app_data.is_empty() {
        return Vec::new();
    }

    let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) else {
        return Vec::new();
    };
    let mut capabilities = Vec::new();
    if let Some(entries) = value.as_array() {
        if entries.len() >= 3 && parse_bool_capability_flag(&entries[2]) {
            capabilities.push("propagation".to_string());
        }
        for entry in entries {
            if let Some(parsed) = extract_capabilities_from_msgpack(entry) {
                capabilities.extend(parsed);
            }
        }
    } else if let Some(parsed) = extract_capabilities_from_msgpack(&value) {
        capabilities.extend(parsed);
    }

    normalize_capabilities(capabilities)
}

fn parse_bool_capability_flag(value: &MsgPackValue) -> bool {
    match value {
        MsgPackValue::Boolean(true) => true,
        MsgPackValue::Integer(value) => value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
            .is_some_and(|value| value == 1),
        MsgPackValue::F64(value) => *value == 1.0,
        MsgPackValue::F32(value) => f64::from(*value) == 1.0,
        MsgPackValue::Binary(text) => parse_fuzzy_bool(std::str::from_utf8(text).ok()),
        MsgPackValue::String(text) => parse_fuzzy_bool(text.as_str()),
        _ => false,
    }
}

fn parse_fuzzy_bool(text: Option<&str>) -> bool {
    match text.map(str::trim).map(str::to_lowercase).as_deref() {
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        _ => false,
    }
}

fn parse_text_to_u32(text: &str) -> Option<u32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = trimmed.parse::<u32>() {
        return Some(value);
    }

    parse_f64_to_u32(trimmed.parse::<f64>().ok()?)
}

fn parse_f64_to_u32(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }

    if value > u32::MAX as f64 {
        return None;
    }

    Some(value as u32)
}

fn parse_fuzzy_u32(value: &MsgPackValue) -> Option<u32> {
    match value {
        MsgPackValue::Integer(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
            .or_else(|| value.as_f64().and_then(parse_f64_to_u32)),
        MsgPackValue::F64(value) => parse_f64_to_u32(*value),
        MsgPackValue::F32(value) => parse_f64_to_u32(f64::from(*value)),
        MsgPackValue::Boolean(value) => Some(u32::from(*value)),
        MsgPackValue::Binary(bytes) => parse_text_to_u32(std::str::from_utf8(bytes).ok()?),
        MsgPackValue::String(text) => parse_text_to_u32(text.as_str()?),
        _ => None,
    }
}

fn parse_announce_costs_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> (Option<u32>, Option<u32>) {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return (None, None);
    };
    let Ok(app_data) = hex::decode(raw_hex) else {
        return (None, None);
    };
    let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) else {
        return (None, None);
    };
    let Some(entries) = value.as_array() else {
        return (None, None);
    };
    let Some(costs) = entries.get(5) else {
        return (None, None);
    };
    if let MsgPackValue::Array(values) = costs {
        return (values.get(1).and_then(parse_fuzzy_u32), values.get(2).and_then(parse_fuzzy_u32));
    }
    let MsgPackValue::Map(entries) = costs else {
        return (None, None);
    };
    let mut stamp_cost_flexibility = None;
    let mut peering_cost = None;
    for (key, value) in entries {
        let Some(key) = msgpack_key_to_string(key) else {
            continue;
        };
        if key == "stamp_cost_flexibility" {
            stamp_cost_flexibility = parse_fuzzy_u32(value);
        }
        if key == "peering_cost" {
            peering_cost = parse_fuzzy_u32(value);
        }
    }
    (stamp_cost_flexibility, peering_cost)
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

pub fn handle_framed_request(daemon: &RpcDaemon, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    daemon.handle_framed_request(bytes)
}
