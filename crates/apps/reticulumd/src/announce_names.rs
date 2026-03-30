pub fn encode_delivery_display_name_app_data(display_name: &str) -> Option<Vec<u8>> {
    let normalized = normalize_display_name(display_name)?;
    let peer_data =
        rmpv::Value::Array(vec![rmpv::Value::Binary(normalized.into_bytes()), rmpv::Value::Nil]);
    rmp_serde::to_vec(&peer_data).ok()
}

pub fn normalize_display_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().any(char::is_control) {
        return None;
    }
    let normalized: String = trimmed.chars().take(64).collect();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn parse_peer_name_from_app_data(app_data: &[u8]) -> Option<(String, &'static str)> {
    if app_data.is_empty() {
        return None;
    }

    if is_msgpack_array_prefix(app_data[0]) {
        if let Some(name) =
            display_name_from_app_data(app_data).and_then(|value| normalize_display_name(&value))
        {
            return Some((name, "delivery_app_data"));
        }
    }

    if let Some(name) =
        pn_name_from_app_data(app_data).and_then(|value| normalize_display_name(&value))
    {
        return Some((name, "pn_meta"));
    }

    let text = std::str::from_utf8(app_data).ok()?;
    let name = normalize_display_name(text)?;
    Some((name, "app_data_utf8"))
}

pub fn lxmf_aspect_from_name_hash(name_hash: &[u8]) -> Option<String> {
    let delivery = rns_transport::destination::DestinationName::new("lxmf", "delivery");
    if name_hash == delivery.as_name_hash_slice() {
        return Some("lxmf.delivery".to_string());
    }

    let propagation = rns_transport::destination::DestinationName::new("lxmf", "propagation");
    if name_hash == propagation.as_name_hash_slice() {
        return Some("lxmf.propagation".to_string());
    }

    let control = rns_transport::destination::DestinationName::new("lxmf", "propagation.control");
    if name_hash == control.as_name_hash_slice() {
        return Some("lxmf.propagation.control".to_string());
    }

    None
}

pub fn pn_stamp_cost_from_app_data(data: &[u8]) -> Option<u32> {
    parse_announce_cost_from_app_data(data, 0)
}

pub fn pn_stamp_cost_flexibility_from_app_data(data: &[u8]) -> Option<u32> {
    parse_announce_cost_from_app_data(data, 1)
}

pub fn pn_peering_cost_from_app_data(data: &[u8]) -> Option<u32> {
    parse_announce_cost_from_app_data(data, 2)
}

fn is_msgpack_array_prefix(byte: u8) -> bool {
    (0x90..=0x9f).contains(&byte) || byte == 0xdc || byte == 0xdd
}

fn display_name_from_app_data(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    if is_msgpack_array_prefix(data[0]) {
        let decoded: rmpv::Value = rmp_serde::from_slice(data).ok()?;
        let entries = match decoded {
            rmpv::Value::Array(entries) => entries,
            _ => return None,
        };

        let first = entries.first()?;
        match first {
            rmpv::Value::Nil => None,
            rmpv::Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
            rmpv::Value::String(text) => text.as_str().map(|value| value.to_string()),
            _ => None,
        }
    } else {
        std::str::from_utf8(data).ok().map(|value| value.to_string())
    }
}

fn pn_name_from_app_data(data: &[u8]) -> Option<String> {
    const PN_META_NAME: u8 = 0x01;

    let decoded = rmp_serde::from_slice::<rmpv::Value>(data).ok()?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => return None,
    };

    let metadata = entries.get(6)?;
    let rmpv::Value::Map(entries) = metadata else {
        return None;
    };

    let name_keys = [
        rmpv::Value::from(PN_META_NAME),
        rmpv::Value::from("name"),
        rmpv::Value::from("n"),
        rmpv::Value::from("display_name"),
    ];

    for (entry_key, entry_value) in entries {
        if name_keys.iter().any(|candidate| keys_match(entry_key, candidate)) {
            return string_like_value_to_string(entry_value);
        }
    }

    None
}

fn keys_match(candidate: &rmpv::Value, expected: &rmpv::Value) -> bool {
    match (candidate, expected) {
        (rmpv::Value::Integer(candidate), rmpv::Value::Integer(expected)) => {
            candidate.as_u64() == expected.as_u64()
        }
        (rmpv::Value::String(candidate), rmpv::Value::String(expected)) => {
            candidate.as_str().is_some_and(|candidate| {
                candidate.eq_ignore_ascii_case(expected.as_str().unwrap_or_default())
            })
        }
        (rmpv::Value::Binary(candidate), rmpv::Value::String(expected)) => {
            std::str::from_utf8(candidate).ok().is_some_and(|candidate| {
                candidate.eq_ignore_ascii_case(expected.as_str().unwrap_or_default().trim())
            })
        }
        (rmpv::Value::String(candidate), rmpv::Value::Binary(expected)) => {
            candidate.as_str().is_some_and(|candidate| {
                std::str::from_utf8(expected.as_slice())
                    .is_ok_and(|expected_key| candidate.trim().eq_ignore_ascii_case(expected_key))
            })
        }
        _ => false,
    }
}

fn string_like_value_to_string(value: &rmpv::Value) -> Option<String> {
    match value {
        rmpv::Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        rmpv::Value::String(text) => text.as_str().map(|s| s.to_string()),
        rmpv::Value::Integer(value) => value.as_i64().map(|value| value.to_string()),
        rmpv::Value::F64(value) => {
            if value.fract() == 0.0 {
                Some(format!("{value:.0}"))
            } else {
                None
            }
        }
        rmpv::Value::F32(value) => {
            let value = f64::from(*value);
            if value.fract() == 0.0 {
                Some(format!("{value:.0}"))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_announce_cost_from_app_data(data: &[u8], index: usize) -> Option<u32> {
    if index > 2 {
        return None;
    }

    let decoded = rmp_serde::from_slice::<rmpv::Value>(data).ok()?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => return None,
    };

    match entries.get(5)? {
        rmpv::Value::Array(costs) => costs.get(index).and_then(rmp_value_to_u32),
        rmpv::Value::Map(costs) => parse_announce_cost_from_map(costs, index),
        _ => None,
    }
}

fn parse_announce_cost_from_map(costs: &[(rmpv::Value, rmpv::Value)], index: usize) -> Option<u32> {
    let target_key = match index {
        0 => ["stamp_cost", "0"],
        1 => ["stamp_cost_flexibility", "1"],
        2 => ["peering_cost", "2"],
        _ => return None,
    };

    costs.iter().find_map(|(key, value)| {
        let cost_key = cost_map_key_text(key)?;
        target_key.contains(&cost_key.as_str()).then(|| rmp_value_to_u32(value)).flatten()
    })
}

fn cost_map_key_text(key: &rmpv::Value) -> Option<String> {
    match key {
        rmpv::Value::String(text) => text.as_str().map(|key| key.trim().to_ascii_lowercase()),
        rmpv::Value::Binary(bytes) => {
            String::from_utf8(bytes.clone()).ok().map(|key| key.trim().to_ascii_lowercase())
        }
        rmpv::Value::Integer(value) => value
            .as_u64()
            .map(|key| key.to_string())
            .or_else(|| value.as_i64().map(|key| key.to_string())),
        _ => None,
    }
}

fn rmp_value_to_u32(value: &rmpv::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
        .or_else(|| match value {
            rmpv::Value::F64(value) => parse_f64_to_u32(*value),
            rmpv::Value::F32(value) => parse_f64_to_u32(f64::from(*value)),
            rmpv::Value::Boolean(value) => Some(u32::from(*value)),
            rmpv::Value::Binary(bytes) => parse_text_to_u32(std::str::from_utf8(bytes).ok()?),
            rmpv::Value::String(text) => parse_text_to_u32(text.as_str()?),
            _ => None,
        })
}

fn parse_f64_to_u32(value: f64) -> Option<u32> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > u32::MAX as f64 {
        return None;
    }
    Some(value as u32)
}

fn parse_text_to_u32(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok()
}
