use rmpv::Value;

use crate::constants::PN_META_NAME;

pub const MAX_DISPLAY_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayNameError {
    Empty,
    ControlChars,
}

pub fn pn_announce_data_is_valid(data: &[u8]) -> bool {
    let decoded = match decode_announce_data(data) {
        Some(decoded) => decoded,
        None => return false,
    };

    if decoded.len() < 6 {
        return false;
    }

    if value_to_u64(&decoded[1]).is_none() {
        return false;
    }

    if value_to_bool(&decoded[2]).is_none() {
        return false;
    }

    if value_to_u64(&decoded[3]).is_none() || value_to_u64(&decoded[4]).is_none() {
        return false;
    }

    match &decoded[5] {
        Value::Array(costs) => {
            if costs.is_empty() {
                return false;
            }
            if rmp_value_to_u32(costs.first().expect("costs is not empty")).is_none() {
                return false;
            }
            if costs.get(1).is_some_and(|value| rmp_value_to_u32(value).is_none()) {
                return false;
            }
            if costs.get(2).is_some_and(|value| rmp_value_to_u32(value).is_none()) {
                return false;
            }
            true
        }
        Value::Map(costs) => {
            if parse_announce_cost_from_map(costs, 0).is_none() {
                return false;
            }
            if cost_map_contains_key(costs, 1) && parse_announce_cost_from_map(costs, 1).is_none() {
                return false;
            }
            if cost_map_contains_key(costs, 2) && parse_announce_cost_from_map(costs, 2).is_none() {
                return false;
            }
            true
        }
        _ => false,
    }
}

pub fn pn_name_from_app_data(data: &[u8]) -> Option<String> {
    let decoded = decode_announce_data(data)?;
    if decoded.len() < 7 {
        return None;
    }

    let metadata = match decoded.get(6)? {
        Value::Map(entries) => entries,
        _ => return None,
    };

    let name_keys = [
        Value::from(PN_META_NAME),
        Value::from("name"),
        Value::from("n"),
        Value::from("display_name"),
    ];

    for (entry_key, entry_value) in metadata {
        if !name_keys.iter().any(|candidate| keys_match(entry_key, candidate)) {
            continue;
        }

        return string_like_value_to_string(entry_value);
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

fn parse_announce_cost_from_app_data(data: &[u8], index: usize) -> Option<u32> {
    if index > 2 {
        return None;
    }

    let decoded = decode_announce_data(data)?;
    match decoded.get(5)? {
        Value::Array(costs) => costs.get(index).and_then(rmp_value_to_u32),
        Value::Map(costs) => parse_announce_cost_from_map(costs, index),
        _ => None,
    }
}

fn parse_announce_cost_from_map(costs: &[(Value, Value)], index: usize) -> Option<u32> {
    let target_key = match index {
        0 => ["stamp_cost", "0"],
        1 => ["stamp_cost_flexibility", "1"],
        2 => ["peering_cost", "2"],
        _ => return None,
    };
    costs.iter().find_map(|(key, value)| {
        let cost_key = cost_map_key_text(key)?;
        target_key.contains(&cost_key.as_str()).then_some(()).and_then(|_| rmp_value_to_u32(value))
    })
}

fn cost_map_contains_key(costs: &[(Value, Value)], index: usize) -> bool {
    let target_key = match index {
        0 => ["stamp_cost", "0"],
        1 => ["stamp_cost_flexibility", "1"],
        2 => ["peering_cost", "2"],
        _ => return false,
    };

    costs.iter().any(|(key, _)| {
        cost_map_key_text(key).is_some_and(|cost_key| target_key.contains(&cost_key.as_str()))
    })
}

fn cost_map_key_text(key: &Value) -> Option<String> {
    match key {
        Value::String(key) => key.as_str().map(|key| key.trim().to_ascii_lowercase()),
        Value::Binary(key) => {
            String::from_utf8(key.clone()).ok().map(|key| key.trim().to_ascii_lowercase())
        }
        Value::Integer(key) => key.as_u64().map(|key| key.to_string()).or_else(|| {
            key.as_i64().and_then(|key| usize::try_from(key).ok()).map(|key| key.to_string())
        }),
        _ => None,
    }
}

fn rmp_value_to_u32(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_i64().and_then(|value| u32::try_from(value).ok()))
        .or_else(|| match value {
            Value::F64(value) => parse_f64_to_u32(*value),
            Value::F32(value) => parse_f64_to_u32(f64::from(*value)),
            Value::Boolean(value) => Some(u32::from(*value)),
            Value::Binary(bytes) => parse_text_to_u32(std::str::from_utf8(bytes).ok()?),
            Value::String(text) => parse_text_to_u32(text.as_str()?),
            _ => None,
        })
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

fn decode_announce_data(data: &[u8]) -> Option<Vec<Value>> {
    if data.is_empty() {
        return None;
    }

    rmp_serde::from_slice(data).ok()
}

fn keys_match(candidate: &Value, expected: &Value) -> bool {
    match (candidate, expected) {
        (Value::Integer(candidate_value), Value::Integer(expected_value)) => {
            candidate_value.as_u64() == expected_value.as_u64()
        }
        (Value::String(candidate_key), Value::String(expected_key)) => {
            candidate_key.as_str().is_some_and(|candidate| {
                candidate.eq_ignore_ascii_case(expected_key.as_str().unwrap_or_default())
            })
        }
        (Value::Binary(candidate_bytes), Value::String(expected_text)) => {
            std::str::from_utf8(candidate_bytes).ok().is_some_and(|candidate_key| {
                candidate_key
                    .eq_ignore_ascii_case(expected_text.as_str().unwrap_or_default().trim())
            })
        }
        (Value::String(candidate_text), Value::Binary(expected_bytes)) => {
            candidate_text.as_str().is_some_and(|candidate| {
                std::str::from_utf8(expected_bytes.as_slice())
                    .is_ok_and(|expected_key| candidate.trim().eq_ignore_ascii_case(expected_key))
            })
        }
        _ => false,
    }
}

fn string_like_value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        Value::String(text) => text.as_str().map(|s| s.to_string()),
        Value::Integer(int) => int.as_i64().map(|value| value.to_string()),
        Value::F64(value) => {
            if value.fract() == 0.0 {
                Some(format!("{value:.0}"))
            } else {
                None
            }
        }
        Value::F32(value) => {
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

fn value_to_u64(value: &Value) -> Option<u64> {
    fn parse_fuzzy_u64(text: &str) -> Option<u64> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Ok(parsed) = trimmed.parse::<u64>() {
            return Some(parsed);
        }

        let parsed = trimmed.parse::<f64>().ok()?;
        if !parsed.is_finite() || parsed.fract() != 0.0 || parsed < 0.0 {
            return None;
        }

        if parsed > u64::MAX as f64 {
            None
        } else {
            Some(parsed as u64)
        }
    }

    match value {
        Value::Integer(int) => {
            int.as_u64().or_else(|| int.as_i64().and_then(|v| u64::try_from(v).ok()))
        }
        Value::F64(value) => {
            if value.fract() != 0.0 || *value < 0.0 {
                return None;
            }

            if !value.is_finite() || *value > u64::MAX as f64 {
                return None;
            }

            Some(*value as u64)
        }
        Value::F32(value) => {
            let value = f64::from(*value);
            if value.fract() != 0.0 || value < 0.0 {
                return None;
            }

            if !value.is_finite() || value > u64::MAX as f64 {
                return None;
            }

            Some(value as u64)
        }
        Value::Binary(bytes) => parse_fuzzy_u64(std::str::from_utf8(bytes).ok()?),
        Value::String(text) => parse_fuzzy_u64(text.as_str()?),
        _ => None,
    }
}

fn value_to_bool(value: &Value) -> Option<bool> {
    fn parse_fuzzy_bool(text: &str) -> Option<bool> {
        match text.trim().to_lowercase().as_str() {
            "0" | "false" | "no" | "off" => Some(false),
            "1" | "true" | "yes" | "on" => Some(true),
            _ => None,
        }
    }

    match value {
        Value::Boolean(value) => Some(*value),
        Value::Integer(int) => match int.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        Value::F64(value) => {
            if value.is_nan() {
                None
            } else if *value == 0.0 {
                Some(false)
            } else if *value == 1.0 {
                Some(true)
            } else {
                None
            }
        }
        Value::F32(value) => {
            let value = f64::from(*value);
            if value.is_nan() {
                None
            } else if value == 0.0 {
                Some(false)
            } else if value == 1.0 {
                Some(true)
            } else {
                None
            }
        }
        Value::Binary(bytes) => parse_fuzzy_bool(std::str::from_utf8(bytes).ok()?),
        Value::String(text) => parse_fuzzy_bool(text.as_str()?),
        _ => None,
    }
}

pub fn display_name_from_app_data(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    if is_msgpack_array_prefix(data[0]) {
        let decoded: Value = rmp_serde::from_slice(data).ok()?;
        let entries = match decoded {
            Value::Array(entries) => entries,
            _ => return None,
        };

        let first = entries.first()?;
        return match first {
            Value::Nil => None,
            Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
            Value::String(text) => text.as_str().map(|s| s.to_string()),
            _ => None,
        };
    }

    std::str::from_utf8(data).ok().map(|s| s.to_string())
}

pub fn normalize_display_name(value: &str) -> Result<String, DisplayNameError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DisplayNameError::Empty);
    }

    if trimmed.chars().any(char::is_control) {
        return Err(DisplayNameError::ControlChars);
    }

    let normalized: String = trimmed.chars().take(MAX_DISPLAY_NAME_CHARS).collect();
    if normalized.is_empty() {
        Err(DisplayNameError::Empty)
    } else {
        Ok(normalized)
    }
}

pub fn is_msgpack_array_prefix(byte: u8) -> bool {
    (0x90..=0x9f).contains(&byte) || byte == 0xdc || byte == 0xdd
}
