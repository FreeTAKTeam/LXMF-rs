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
