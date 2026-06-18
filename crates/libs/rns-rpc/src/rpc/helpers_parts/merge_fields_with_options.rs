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

fn outbound_wire_fields(fields: Option<JsonValue>) -> Option<JsonValue> {
    let Some(JsonValue::Object(map)) = fields else {
        return None;
    };

    let wire = map
        .into_iter()
        .filter(|(key, _)| !is_private_outbound_field_key(key))
        .collect::<JsonMap<String, JsonValue>>();
    (!wire.is_empty()).then_some(JsonValue::Object(wire))
}

fn is_private_outbound_field_key(key: &str) -> bool {
    matches!(
        key,
        "_lxmf" | "_sdk" | "_fields_raw" | "title" | "content" | "body" | "payload"
    )
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

    if let Ok(value) = ciborium::de::from_reader::<JsonValue, _>(payload) {
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

fn parse_fuzzy_nonnegative_f64(value: &MsgPackValue) -> Option<f64> {
    let parsed = match value {
        MsgPackValue::Integer(value) => value
            .as_f64()
            .or_else(|| value.as_i64().map(|value| value as f64))
            .or_else(|| value.as_u64().map(|value| value as f64))?,
        MsgPackValue::F64(value) => *value,
        MsgPackValue::F32(value) => f64::from(*value),
        MsgPackValue::Boolean(value) => f64::from(u8::from(*value)),
        MsgPackValue::Binary(bytes) => std::str::from_utf8(bytes).ok()?.trim().parse().ok()?,
        MsgPackValue::String(text) => text.as_str()?.trim().parse().ok()?,
        _ => return None,
    };
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed)
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

    let transfer_limit_bytes = entries.get(3).and_then(parse_fuzzy_nonnegative_f64).and_then(|limit| {
        let bytes = limit * 1000.0;
        (bytes.is_finite() && bytes <= u32::MAX as f64).then_some(bytes as u32)
    });
    let sync_limit_bytes = match (
        transfer_limit_bytes,
        entries.get(4).and_then(parse_fuzzy_nonnegative_f64).and_then(|limit| {
            let bytes = limit * 1000.0;
            (bytes.is_finite() && bytes <= u32::MAX as f64).then_some(bytes as u32)
        }),
    ) {
        (Some(transfer), Some(sync)) if sync < transfer => Some(transfer),
        (_, sync) => sync,
    };

    (transfer_limit_bytes, sync_limit_bytes)
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

fn parse_propagation_metadata_from_app_data_hex(app_data_hex: Option<&str>) -> JsonValue {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return JsonValue::Null;
    };
    let Ok(app_data) = hex::decode(raw_hex) else {
        return JsonValue::Null;
    };
    let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) else {
        return JsonValue::Null;
    };
    let Some(metadata) = value.as_array().and_then(|entries| entries.get(6)) else {
        return JsonValue::Null;
    };
    pn_metadata_to_json(metadata).unwrap_or(JsonValue::Null)
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
