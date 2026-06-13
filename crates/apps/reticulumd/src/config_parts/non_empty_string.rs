fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn string_from_value(
    value: toml::Value,
    key: &str,
    index: usize,
    kind: &str,
) -> Result<String, String> {
    value
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| format!("interfaces[{index}].{key} must be a string for {kind}"))
}

fn port_number_from_value(value: toml::Value, index: usize) -> Result<u16, String> {
    value
        .as_integer()
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| format!("interfaces[{index}].port must be a 16-bit integer"))
}

fn deserialize_optional_string_list<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<toml::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        toml::Value::String(value) => Ok(Some(split_string_list(&value))),
        toml::Value::Array(items) => items
            .into_iter()
            .map(|item| {
                item.as_str().map(str::to_string).ok_or_else(|| {
                    D::Error::custom("interface device list entries must be strings")
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        _ => Err(D::Error::custom("interface device list must be a string or string array")),
    }
}

fn split_string_list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|value| !value.is_empty()).map(str::to_string).collect()
}

fn normalize_interface_kind(value: &str) -> String {
    match value {
        "AutoInterface" => "auto".to_string(),
        "TCPClientInterface" => "tcp_client".to_string(),
        "TCPServerInterface" => "tcp_server".to_string(),
        "UDPInterface" => "udp".to_string(),
        "SerialInterface" => "serial".to_string(),
        "KISSInterface" => "kiss".to_string(),
        "RNodeInterface" => "lora".to_string(),
        "Vrn76KissBluetoothInterface" | "Vrn76KissBleInterface" => "vrn76_kiss_ble".to_string(),
        value => value.to_string(),
    }
}

fn is_known_unsupported_python_interface(value: &str) -> bool {
    matches!(
        value,
        "PipeInterface"
            | "LocalInterface"
            | "I2PInterface"
            | "WeaveInterface"
            | "BackboneInterface"
    )
}

fn is_tcp_lora_port(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("tcp://")
}

fn is_ble_lora_port(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("ble://")
}

fn deserialize_interfaces<'de, D>(deserializer: D) -> Result<Vec<InterfaceConfig>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = toml::Value::deserialize(deserializer)?;
    match value {
        toml::Value::Array(items) => items.into_iter().map(interface_from_value).collect(),
        toml::Value::Table(table) => table
            .into_iter()
            .map(|(key, value)| {
                let toml::Value::Table(mut interface) = value else {
                    return Err(D::Error::custom(format!(
                        "interfaces.{key} must be an interface settings table"
                    )));
                };
                if !interface.contains_key("name") {
                    interface.insert("name".to_string(), toml::Value::String(key.clone()));
                }
                if !interface.contains_key("type") {
                    interface.insert("type".to_string(), toml::Value::String(key));
                }
                interface_from_value(toml::Value::Table(interface))
            })
            .collect(),
        other => Err(D::Error::custom(format!(
            "interfaces must be an array or table, got {}",
            other.type_str()
        ))),
    }
}

fn interface_from_value<E>(value: toml::Value) -> Result<InterfaceConfig, E>
where
    E: DeError,
{
    value.try_into().map_err(E::custom)
}

fn require_non_empty(value: Option<&str>, error: &str) -> Result<(), String> {
    if value.is_some_and(|item| !item.trim().is_empty()) {
        Ok(())
    } else {
        Err(error.to_string())
    }
}

fn insert_opt_string(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<&String>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::String(value.clone()));
    }
}

fn insert_opt_string_array(
    target: &mut JsonMap<String, JsonValue>,
    key: &str,
    value: Option<&Vec<String>>,
) {
    if let Some(value) = value {
        target.insert(
            key.to_string(),
            JsonValue::Array(value.iter().cloned().map(JsonValue::String).collect()),
        );
    }
}

fn insert_opt_u64(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::Number(value.into()));
    }
}

fn insert_opt_bool(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        target.insert(key.to_string(), JsonValue::Bool(value));
    }
}

fn insert_opt_f64(target: &mut JsonMap<String, JsonValue>, key: &str, value: Option<f64>) {
    if let Some(value) = value.and_then(serde_json::Number::from_f64) {
        target.insert(key.to_string(), JsonValue::Number(value));
    }
}

fn matches_normalized(value: &str, candidates: &[&str]) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    candidates.iter().any(|candidate| normalized == *candidate)
}

fn matches_vrn76_frame_mode(value: &str) -> bool {
    matches_normalized(value, &["benshi_tnc_data", "benshi", "raw_kiss", "raw"])
}

fn is_uuid_like(value: &str) -> bool {
    let normalized = value.trim();
    if normalized.is_empty() {
        return false;
    }

    if normalized.len() == 4 || normalized.len() == 8 {
        return normalized.chars().all(|ch| ch.is_ascii_hexdigit());
    }

    if normalized.len() == 36 {
        let bytes = normalized.as_bytes();
        let hyphen_positions = [8_usize, 13, 18, 23];
        for idx in hyphen_positions {
            if bytes[idx] != b'-' {
                return false;
            }
        }
        return normalized
            .chars()
            .enumerate()
            .all(|(idx, ch)| hyphen_positions.contains(&idx) || ch.is_ascii_hexdigit());
    }

    false
}

fn is_supported_lora_region(region: &str) -> bool {
    matches!(
        region.trim().to_ascii_uppercase().as_str(),
        "EU868" | "US915" | "AU915" | "AS923" | "IN865" | "KR920" | "RU864"
    )
}
