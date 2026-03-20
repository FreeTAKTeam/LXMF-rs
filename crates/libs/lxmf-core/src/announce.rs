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

pub fn encode_delivery_display_name_app_data(display_name: &str) -> Option<Vec<u8>> {
    let normalized = normalize_display_name(display_name)?;
    let peer_data =
        rmpv::Value::Array(vec![rmpv::Value::Binary(normalized.into_bytes()), rmpv::Value::Nil]);
    rmp_serde::to_vec(&peer_data).ok()
}

pub fn display_name_from_delivery_app_data(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    let decoded: rmpv::Value = rmp_serde::from_slice(data).ok()?;
    match decoded {
        rmpv::Value::Array(values) => {
            let first = values.first()?;
            match first {
                rmpv::Value::Binary(bytes) => {
                    let raw = String::from_utf8(bytes.clone()).ok()?;
                    normalize_display_name(raw.as_str())
                }
                rmpv::Value::String(value) => normalize_display_name(value.as_str()?),
                _ => None,
            }
        }
        rmpv::Value::Binary(bytes) => {
            let raw = String::from_utf8(bytes).ok()?;
            normalize_display_name(raw.as_str())
        }
        rmpv::Value::String(value) => normalize_display_name(value.as_str()?),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_decode_delivery_display_name_round_trip() {
        let encoded = encode_delivery_display_name_app_data("Alice Router").expect("encoded");
        let decoded = display_name_from_delivery_app_data(encoded.as_slice()).expect("decoded");
        assert_eq!(decoded, "Alice Router");
    }

    #[test]
    fn normalize_display_name_rejects_control_bytes() {
        assert!(normalize_display_name("Alice\nRouter").is_none());
    }
}
