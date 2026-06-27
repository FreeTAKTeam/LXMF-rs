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

fn outbound_wire_fields(fields: Option<JsonValue>) -> Result<Option<JsonValue>, &'static str> {
    let Some(fields) = fields else {
        return Ok(None);
    };
    let JsonValue::Object(map) = fields else {
        return Err("outbound fields is not a JSON object");
    };

    let wire = map
        .into_iter()
        .filter(|(key, _)| !is_private_outbound_field_key(key))
        .collect::<JsonMap<String, JsonValue>>();
    Ok((!wire.is_empty()).then_some(JsonValue::Object(wire)))
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

    match parse_rch_capabilities_from_lxmf_announce(&app_data) {
        Ok(Some(capabilities)) => return capabilities,
        Ok(None) => {}
        Err(err) => {
            log::debug!("[daemon] failed to parse RCH capabilities from announce app_data: {err}");
        }
    }

    if let Ok(value) = rmp_serde::from_slice::<MsgPackValue>(&app_data) {
        let mut capabilities = Vec::new();
        if let Some(entries) = value.as_array() {
            if entries.len() >= 3 && parse_bool_capability_flag(&entries[2]) {
                capabilities.push("propagation".to_string());
            }
            for entry in entries {
                match extract_capabilities_from_msgpack(entry) {
                    Ok(Some(parsed)) => capabilities.extend(parsed),
                    Ok(None) => {}
                    Err(err) => {
                        log::debug!("[daemon] failed to extract capabilities from msgpack: {err}");
                    }
                }
            }
        } else {
            match extract_capabilities_from_msgpack(&value) {
                Ok(Some(parsed)) => capabilities.extend(parsed),
                Ok(None) => {}
                Err(err) => {
                    log::debug!("[daemon] failed to extract capabilities from msgpack: {err}");
                }
            }
        }
        let capabilities = normalize_capabilities(capabilities);
        if !capabilities.is_empty() {
            return capabilities;
        }
    }

    parse_capabilities_from_utf8_app_data(&app_data)
}

fn parse_rch_capabilities_from_lxmf_announce(
    app_data: &[u8],
) -> Result<Option<Vec<String>>, &'static str> {
    let value = rmp_serde::from_slice::<MsgPackValue>(app_data)
        .map_err(|_| "malformed msgpack in LXMF announce app_data")?;
    let Some(entries) = value.as_array() else {
        return Ok(None);
    };
    let capability_payload = match entries.get(2) {
        Some(MsgPackValue::Binary(bytes)) => bytes.as_slice(),
        Some(MsgPackValue::String(text)) => {
            text.as_str().ok_or("non-UTF-8 msgpack string in capability slot")?.as_bytes()
        }
        _ => return Ok(None),
    };

    let capabilities = parse_rch_capability_payload(capability_payload);
    Ok((!capabilities.is_empty()).then_some(capabilities))
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
            capabilities = extract_capabilities_from_msgpack(value)
            .inspect_err(|err| {
                log::debug!("[daemon] failed to extract RCH capabilities from msgpack: {err}");
            })
            .ok()
            .flatten()
            .unwrap_or_default();
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
        MsgPackValue::Binary(text) => parse_fuzzy_bool(decode_utf8(text, "app_data field").ok()),
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

fn parse_text_to_u32(text: &str) -> Result<u32, &'static str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty text");
    }

    if let Ok(value) = trimmed.parse::<u32>() {
        return Ok(value);
    }

    let f = trimmed.parse::<f64>().map_err(|_| "invalid number string")?;
    parse_f64_to_u32(f)
}

fn parse_f64_to_u32(value: f64) -> Result<u32, &'static str> {
    if !value.is_finite() {
        return Err("non-finite float");
    }
    if value < 0.0 {
        return Err("negative float value");
    }
    if value.fract() != 0.0 {
        return Err("fractional float value");
    }
    if value > u32::MAX as f64 {
        return Err("float value out of u32 range");
    }
    Ok(value as u32)
}

fn parse_fuzzy_u32(value: &MsgPackValue) -> Result<u32, &'static str> {
    match value {
        MsgPackValue::Integer(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
            .or_else(|| value.as_f64().and_then(|f| parse_f64_to_u32(f).ok()))
            .ok_or("integer out of u32 range"),
        MsgPackValue::F64(value) => parse_f64_to_u32(*value),
        MsgPackValue::F32(value) => parse_f64_to_u32(f64::from(*value)),
        MsgPackValue::Boolean(value) => Ok(u32::from(*value)),
        MsgPackValue::Binary(bytes) => {
            let text = decode_utf8(bytes, "app_data field").map_err(|_| "invalid UTF-8")?;
            parse_text_to_u32(text)
        }
        MsgPackValue::String(text) => {
            parse_text_to_u32(text.as_str().ok_or("invalid msgpack string")?)
        }
        _ => Err("unsupported msgpack type"),
    }
}

fn parse_fuzzy_nonnegative_f64(value: &MsgPackValue) -> Result<f64, &'static str> {
    let parsed = match value {
        MsgPackValue::Integer(value) => value
            .as_f64()
            .or_else(|| value.as_i64().map(|value| value as f64))
            .or_else(|| value.as_u64().map(|value| value as f64))
            .ok_or("integer overflow in float conversion")?,
        MsgPackValue::F64(value) => *value,
        MsgPackValue::F32(value) => f64::from(*value),
        MsgPackValue::Boolean(value) => f64::from(u8::from(*value)),
        MsgPackValue::Binary(bytes) => {
            let text = decode_utf8(bytes, "app_data field").map_err(|_| "invalid UTF-8")?;
            text.trim().parse::<f64>().map_err(|_| "invalid float string")?
        }
        MsgPackValue::String(text) => {
            text.as_str()
                .ok_or("invalid msgpack string")?
                .trim()
                .parse::<f64>()
                .map_err(|_| "invalid float string")?
        }
        _ => return Err("unsupported msgpack type"),
    };
    if parsed.is_finite() && parsed >= 0.0 {
        Ok(parsed)
    } else {
        Err("non-finite or negative float value")
    }
}

fn parse_fuzzy_i64(value: &MsgPackValue) -> Result<i64, &'static str> {
    match value {
        MsgPackValue::Integer(value) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .ok_or("integer out of i64 range"),
        MsgPackValue::F64(value) => {
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64
            {
                Ok(*value as i64)
            } else {
                Err("float is not a valid i64")
            }
        }
        MsgPackValue::F32(value) => {
            let value = f64::from(*value);
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64
            {
                Ok(value as i64)
            } else {
                Err("float is not a valid i64")
            }
        }
        MsgPackValue::Boolean(value) => Ok(if *value { 1 } else { 0 }),
        MsgPackValue::Binary(bytes) => {
            let text = decode_utf8(bytes, "app_data field").map_err(|_| "invalid UTF-8")?;
            text.trim().parse::<i64>().map_err(|_| "invalid integer string")
        }
        MsgPackValue::String(text) => text
            .as_str()
            .ok_or("invalid msgpack string")?
            .trim()
            .parse::<i64>()
            .map_err(|_| "invalid integer string"),
        _ => Err("unsupported msgpack type"),
    }
}

/// Announce costs parsed from app_data: `(stamp_cost, stamp_cost_flexibility, peering_cost)`.
type AnnounceCosts = (Option<u32>, Option<u32>, Option<u32>);

fn parse_announce_costs_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> Result<AnnounceCosts, &'static str> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((None, None, None));
    };
    let app_data = hex::decode(raw_hex).map_err(|_| "invalid hex in announce app_data")?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data)
        .map_err(|_| "malformed msgpack in announce app_data")?;
    let Some(entries) = value.as_array() else {
        return Ok((None, None, None));
    };
    let Some(costs) = entries.get(5) else {
        return Ok((None, None, None));
    };
    if let MsgPackValue::Array(values) = costs {
        return Ok((
            parse_announce_cost_slot(values.first(), "stamp"),
            parse_announce_cost_slot(values.get(1), "stamp flexibility"),
            parse_announce_cost_slot(values.get(2), "peering"),
        ));
    }
    let MsgPackValue::Map(entries) = costs else {
        return Ok((None, None, None));
    };
    let mut stamp_cost = None;
    let mut stamp_cost_flexibility = None;
    let mut peering_cost = None;
    for (key, value) in entries {
        let Some(key) = msgpack_key_to_string(key) else {
            continue;
        };
        if key == "stamp_cost" {
            stamp_cost = parse_announce_cost_slot(Some(value), "stamp");
        }
        if key == "stamp_cost_flexibility" {
            stamp_cost_flexibility = parse_announce_cost_slot(Some(value), "stamp flexibility");
        }
        if key == "peering_cost" {
            peering_cost = parse_announce_cost_slot(Some(value), "peering");
        }
    }
    Ok((stamp_cost, stamp_cost_flexibility, peering_cost))
}

fn parse_announce_cost_slot(slot: Option<&MsgPackValue>, label: &str) -> Option<u32> {
    let raw = slot?;
    // A missing or nil slot is a legitimate absence; only genuinely malformed/unsupported
    // values are logged. Parsing each slot independently keeps a valid sibling cost from
    // being discarded when one optional slot is bad (the caller collapses Err to all-None).
    if matches!(raw, MsgPackValue::Nil) {
        return None;
    }
    match parse_fuzzy_u32(raw) {
        Ok(cost) => Some(cost),
        Err(err) => {
            log::warn!("[daemon] ignoring malformed announce {label} cost: {err}");
            None
        }
    }
}

fn parse_propagation_limits_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> Result<(Option<u32>, Option<u32>), &'static str> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((None, None));
    };
    let app_data = hex::decode(raw_hex).map_err(|_| "invalid hex in propagation app_data")?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data)
        .map_err(|_| "malformed msgpack in propagation app_data")?;
    let Some(entries) = value.as_array() else {
        return Ok((None, None));
    };

    // Parse each limit slot independently: a malformed sibling slot must not erase a
    // valid value (the caller collapses any Err into `(None, None)`). A per-slot parse
    // failure is logged and treated as absent; structural failures above still propagate.
    let transfer_limit_bytes = parse_propagation_limit_slot(entries.get(3), "transfer");
    let sync_raw = parse_propagation_limit_slot(entries.get(4), "sync");
    let sync_limit_bytes = match (transfer_limit_bytes, sync_raw) {
        (Some(transfer), Some(sync)) if sync < transfer => Some(transfer),
        (_, sync) => sync,
    };

    Ok((transfer_limit_bytes, sync_limit_bytes))
}

fn parse_propagation_limit_slot(slot: Option<&MsgPackValue>, label: &str) -> Option<u32> {
    let raw = slot?;
    match parse_fuzzy_nonnegative_f64(raw) {
        Ok(limit) => {
            let bytes = limit * 1000.0;
            (bytes.is_finite() && bytes <= u32::MAX as f64).then_some(bytes as u32)
        }
        Err(err) => {
            log::warn!("[daemon] ignoring malformed propagation {label} limit: {err}");
            None
        }
    }
}

fn parse_propagation_timebase_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> Result<Option<i64>, &'static str> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let app_data = hex::decode(raw_hex).map_err(|_| "invalid hex in propagation app_data")?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data)
        .map_err(|_| "malformed msgpack in propagation app_data")?;
    let Some(entries) = value.as_array() else {
        return Ok(None);
    };
    entries.get(1).map(parse_fuzzy_i64).transpose()
}

fn parse_propagation_enabled_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> Result<Option<bool>, &'static str> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let app_data = hex::decode(raw_hex).map_err(|_| "invalid hex in propagation app_data")?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data)
        .map_err(|_| "malformed msgpack in propagation app_data")?;
    let Some(entries) = value.as_array() else {
        return Ok(None);
    };
    // The enabled flag lives at index 2 (`[flag, timebase, enabled, ...]`); gate on that
    // slot rather than a later cost slot so minimal announces keep their flag.
    if entries.len() < 3 {
        return Ok(None);
    }
    Ok(entries.get(2).map(parse_bool_capability_flag))
}

fn parse_propagation_metadata_from_app_data_hex(app_data_hex: Option<&str>) -> Result<JsonValue, &'static str> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(JsonValue::Null);
    };
    let app_data = hex::decode(raw_hex).map_err(|_| "invalid hex in propagation metadata")?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data)
        .map_err(|_| "malformed msgpack in propagation metadata")?;
    let Some(metadata) = value.as_array().and_then(|entries| entries.get(6)) else {
        return Ok(JsonValue::Null);
    };
    Ok(pn_metadata_to_json(metadata)?.unwrap_or(JsonValue::Null))
}

fn parse_peer_name_from_app_data_hex(
    app_data_hex: Option<&str>,
) -> Result<Option<(String, &'static str)>, &'static str> {
    let Some(raw_hex) = app_data_hex.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let app_data = hex::decode(raw_hex).map_err(|_| "invalid hex in peer name app_data")?;
    let value = rmp_serde::from_slice::<MsgPackValue>(&app_data)
        .map_err(|_| "malformed msgpack in peer name app_data")?;
    let Some(entries) = value.as_array() else {
        return Ok(None);
    };

    if let Some(name) = entries.get(6).map(parse_pn_metadata_name).transpose()?.flatten() {
        return Ok(Some((name, "pn_meta")));
    }
    if let Some(name) = entries.first().map(msgpack_value_to_clean_name).transpose()?.flatten() {
        return Ok(Some((name, "delivery_app_data")));
    }
    Ok(None)
}
