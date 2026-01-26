use rmpv::Value;

use crate::constants::PN_META_NAME;

pub fn pn_announce_data_is_valid(data: &[u8]) -> bool {
    let decoded: Vec<Value> = match rmp_serde::from_slice(data) {
        Ok(decoded) => decoded,
        Err(_) => return false,
    };

    if decoded.len() < 7 {
        return false;
    }

    if !value_is_int(&decoded[1]) {
        return false;
    }

    if !matches!(decoded[2], Value::Boolean(_)) {
        return false;
    }

    if !value_is_int(&decoded[3]) || !value_is_int(&decoded[4]) {
        return false;
    }

    let stamp_costs = match &decoded[5] {
        Value::Array(values) => values,
        _ => return false,
    };

    if stamp_costs.len() < 3 {
        return false;
    }

    if !value_is_int(&stamp_costs[0])
        || !value_is_int(&stamp_costs[1])
        || !value_is_int(&stamp_costs[2])
    {
        return false;
    }

    matches!(decoded[6], Value::Map(_))
}

pub fn pn_name_from_app_data(data: &[u8]) -> Option<String> {
    if !pn_announce_data_is_valid(data) {
        return None;
    }

    let decoded: Vec<Value> = rmp_serde::from_slice(data).ok()?;
    let metadata = match decoded.get(6)? {
        Value::Map(entries) => entries,
        _ => return None,
    };

    let key = Value::from(PN_META_NAME);
    for (entry_key, entry_value) in metadata {
        if *entry_key != key {
            continue;
        }

        return match entry_value {
            Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
            Value::String(text) => text.as_str().map(|s| s.to_string()),
            _ => None,
        };
    }

    None
}

pub fn pn_stamp_cost_from_app_data(data: &[u8]) -> Option<u32> {
    if !pn_announce_data_is_valid(data) {
        return None;
    }

    let decoded: Vec<Value> = rmp_serde::from_slice(data).ok()?;
    let stamp_costs = match decoded.get(5)? {
        Value::Array(values) => values,
        _ => return None,
    };

    value_to_u32(stamp_costs.get(0)?)
}

fn value_is_int(value: &Value) -> bool {
    value.as_i64().is_some() || value.as_u64().is_some()
}

fn value_to_u32(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|v| u32::try_from(v).ok()).or_else(|| {
        value
            .as_i64()
            .and_then(|v| if v >= 0 { u32::try_from(v).ok() } else { None })
    })
}
