pub fn validated_binary_id_list(values: &[rmpv::Value]) -> Option<Vec<Vec<u8>>> {
    values
        .iter()
        .map(|value| match value {
            rmpv::Value::Binary(bytes) if bytes.len() == 32 => Some(bytes.clone()),
            _ => None,
        })
        .collect()
}

pub fn parse_transfer_limit_bytes(value: &rmpv::Value) -> Result<Option<usize>, &'static str> {
    let limit: f64 = match value {
        rmpv::Value::F64(value) => *value,
        rmpv::Value::F32(value) => f64::from(*value),
        rmpv::Value::Integer(value) => {
            value.as_f64().ok_or("transfer limit integer out of f64 range")?
        }
        rmpv::Value::String(value) => value
            .as_str()
            .ok_or("transfer limit string not valid UTF-8")?
            .trim()
            .parse::<f64>()
            .map_err(|_| "transfer limit not a valid float")?,
        rmpv::Value::Binary(value) => std::str::from_utf8(value)
            .map_err(|_| "transfer limit bytes not valid UTF-8")?
            .trim()
            .parse::<f64>()
            .map_err(|_| "transfer limit not a valid float")?,
        rmpv::Value::Boolean(value) => f64::from(*value as u8),
        _ => return Err("unsupported value type for transfer limit"),
    };
    if limit.is_nan() {
        return Err("transfer limit is NaN");
    }
    if limit.is_infinite() && limit.is_sign_positive() {
        return Ok(None);
    }
    Ok(Some((limit.max(0.0) * 1000.0) as usize))
}
