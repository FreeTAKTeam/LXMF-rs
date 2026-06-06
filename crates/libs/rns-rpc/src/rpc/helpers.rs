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

    let mut lxmf = match root.remove("_lxmf") {
        Some(JsonValue::Object(map)) => map,
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("_raw".into(), other);
            map
        }
        None => JsonMap::new(),
    };
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

fn now_millis_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0)
}

fn now_seconds_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
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

    if let Some(capabilities) = parse_rch_capabilities_from_lxmf_announce(&app_data) {
        return capabilities;
    }

    if let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) {
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
        let capabilities = normalize_capabilities(capabilities);
        if !capabilities.is_empty() {
            return capabilities;
        }
    }

    parse_capabilities_from_utf8_app_data(&app_data)
}

fn parse_rch_capabilities_from_lxmf_announce(app_data: &[u8]) -> Option<Vec<String>> {
    let value = rmp_serde::from_slice::<MsgPackValue>(app_data).ok()?;
    let entries = value.as_array()?;
    let capability_payload = match entries.get(2) {
        Some(MsgPackValue::Binary(bytes)) => bytes.as_slice(),
        Some(MsgPackValue::String(text)) => text.as_str()?.as_bytes(),
        _ => return None,
    };

    let capabilities = parse_rch_capability_payload(capability_payload);
    (!capabilities.is_empty()).then_some(capabilities)
}

fn parse_rch_capability_payload(payload: &[u8]) -> Vec<String> {
    if payload.is_empty() {
        return Vec::new();
    }

    if let Ok(value) = serde_cbor::from_slice::<JsonValue>(payload) {
        let capabilities = extract_rch_capabilities_from_json_value(&value);
        if !capabilities.is_empty() {
            return capabilities;
        }
    }

    if let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(payload) {
        let capabilities = extract_rch_capabilities_from_msgpack_value(&value);
        if !capabilities.is_empty() {
            return capabilities;
        }
    }

    Vec::new()
}

fn extract_rch_capabilities_from_json_value(value: &JsonValue) -> Vec<String> {
    let JsonValue::Object(map) = value else {
        return Vec::new();
    };
    let Some(app) = map.get("app").and_then(JsonValue::as_str) else {
        return Vec::new();
    };
    if !app.eq_ignore_ascii_case("rch") {
        return Vec::new();
    }
    map.get("caps")
        .map(extract_capabilities_from_json_value)
        .unwrap_or_default()
}

fn extract_rch_capabilities_from_msgpack_value(value: &MsgPackValue) -> Vec<String> {
    let MsgPackValue::Map(entries) = value else {
        return Vec::new();
    };

    let mut app_is_rch = false;
    let mut capabilities = Vec::new();
    for (key, value) in entries {
        let Some(name) = msgpack_key_to_string(key) else {
            continue;
        };
        if name == "app" {
            app_is_rch = capability_value_to_string(value)
                .is_some_and(|app| app.eq_ignore_ascii_case("rch"));
        } else if name == "caps" {
            capabilities = extract_capabilities_from_msgpack(value).unwrap_or_default();
        }
    }

    if app_is_rch {
        return capabilities;
    }

    Vec::new()
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

fn parse_fuzzy_i64(value: &MsgPackValue) -> Option<i64> {
    match value {
        MsgPackValue::Integer(value) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok())),
        MsgPackValue::F64(value) => {
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64
            {
                Some(*value as i64)
            } else {
                None
            }
        }
        MsgPackValue::F32(value) => {
            let value = f64::from(*value);
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64
            {
                Some(value as i64)
            } else {
                None
            }
        }
        MsgPackValue::Boolean(value) => Some(if *value { 1 } else { 0 }),
        MsgPackValue::Binary(bytes) => {
            std::str::from_utf8(bytes).ok()?.trim().parse::<i64>().ok()
        }
        MsgPackValue::String(text) => text.as_str()?.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn parse_announce_costs_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> (Option<u32>, Option<u32>, Option<u32>) {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return (None, None, None);
    };
    let Ok(app_data) = hex::decode(raw_hex) else {
        return (None, None, None);
    };
    let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) else {
        return (None, None, None);
    };
    let Some(entries) = value.as_array() else {
        return (None, None, None);
    };
    let Some(costs) = entries.get(5) else {
        return (None, None, None);
    };
    if let MsgPackValue::Array(values) = costs {
        return (
            values.first().and_then(parse_fuzzy_u32),
            values.get(1).and_then(parse_fuzzy_u32),
            values.get(2).and_then(parse_fuzzy_u32),
        );
    }
    let MsgPackValue::Map(entries) = costs else {
        return (None, None, None);
    };
    let mut stamp_cost = None;
    let mut stamp_cost_flexibility = None;
    let mut peering_cost = None;
    for (key, value) in entries {
        let Some(key) = msgpack_key_to_string(key) else {
            continue;
        };
        if key == "stamp_cost" {
            stamp_cost = parse_fuzzy_u32(value);
        }
        if key == "stamp_cost_flexibility" {
            stamp_cost_flexibility = parse_fuzzy_u32(value);
        }
        if key == "peering_cost" {
            peering_cost = parse_fuzzy_u32(value);
        }
    }
    (stamp_cost, stamp_cost_flexibility, peering_cost)
}

fn parse_propagation_limits_from_app_data_hex(
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

    let transfer_limit = entries.get(3).and_then(parse_fuzzy_u32);
    let sync_limit = match (transfer_limit, entries.get(4).and_then(parse_fuzzy_u32)) {
        (Some(transfer), Some(sync)) if sync < transfer => Some(transfer),
        (_, sync) => sync,
    };

    (transfer_limit, sync_limit)
}

fn parse_propagation_timebase_from_app_data_hex(app_data_hex: Option<&str>) -> Option<i64> {
    let raw_hex = app_data_hex.map(str::trim).filter(|value| !value.is_empty())?;
    let app_data = hex::decode(raw_hex).ok()?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data).ok()?;
    let entries = value.as_array()?;
    entries.get(1).and_then(parse_fuzzy_i64)
}

fn parse_propagation_enabled_from_app_data_hex(app_data_hex: Option<&str>) -> Option<bool> {
    let raw_hex = app_data_hex.map(str::trim).filter(|value| !value.is_empty())?;
    let app_data = hex::decode(raw_hex).ok()?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data).ok()?;
    let entries = value.as_array()?;
    if entries.len() < 6 {
        return None;
    }
    entries.get(2).map(parse_bool_capability_flag)
}

fn parse_peer_name_from_app_data_hex(app_data_hex: Option<&str>) -> Option<(String, &'static str)> {
    let raw_hex = app_data_hex.map(str::trim).filter(|value| !value.is_empty())?;
    let app_data = hex::decode(raw_hex).ok()?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data).ok()?;
    let entries = value.as_array()?;

    if let Some(name) = entries.get(6).and_then(parse_pn_metadata_name) {
        return Some((name, "pn_meta"));
    }
    if let Some(name) = entries.first().and_then(msgpack_value_to_clean_name) {
        return Some((name, "delivery_app_data"));
    }
    None
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

#[cfg(test)]
mod tests {
    include!("helpers_tests.rs");
}
