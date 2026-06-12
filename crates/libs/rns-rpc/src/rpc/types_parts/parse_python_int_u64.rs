fn parse_python_int_u64(value: &JsonValue) -> Option<u64> {
    if let Some(value) = value.as_u64() {
        Some(value)
    } else if let Some(value) = value.as_i64() {
        u64::try_from(value.max(0)).ok()
    } else if let Some(value) = value.as_f64() {
        let value = value.max(0.0).trunc();
        (value.is_finite() && value <= u64::MAX as f64).then_some(value as u64)
    } else if let Some(value) = value.as_bool() {
        Some(u64::from(value))
    } else if let Some(value) = value.as_str() {
        u64::try_from(value.trim().parse::<i64>().ok()?.max(0)).ok()
    } else {
        None
    }
}

fn parse_python_int_u8(value: &JsonValue) -> Option<u8> {
    u8::try_from(parse_python_int_u32(value)?).ok()
}

fn parse_python_timestamp_i64(value: &JsonValue) -> Result<i64, &'static str> {
    if let Some(value) = value.as_i64() {
        Ok(value)
    } else if let Some(value) = value.as_u64() {
        i64::try_from(value).map_err(|_| "timestamp exceeds i64 range")
    } else if let Some(value) = value.as_f64() {
        let value = value.trunc();
        if value.is_finite() && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
            Ok(value as i64)
        } else {
            Err("invalid timestamp")
        }
    } else {
        Err("invalid timestamp")
    }
}

fn bytes_to_kilobytes(value: u32) -> f64 {
    f64::from(value) / 1000.0
}

fn bytes_to_python_sync_limit_kilobytes(value: u32) -> u32 {
    if value == 0 {
        0
    } else {
        value.saturating_add(999) / 1000
    }
}

fn default_true() -> bool {
    true
}

fn default_autopeer_maxdepth() -> u32 {
    6
}

fn default_propagation_stamp_cost_flexibility() -> u32 {
    3
}

fn default_delivery_transfer_limit() -> u32 {
    1000
}

fn default_propagation_transfer_limit() -> u32 {
    256
}

fn default_propagation_sync_limit() -> u32 {
    10240
}

fn default_network_distance() -> u32 {
    1
}

fn default_peer_sync_strategy() -> u8 {
    2
}
