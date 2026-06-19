pub fn encode_delivery_display_name_app_data(display_name: &str) -> Option<Vec<u8>> {
    encode_delivery_announce_app_data(display_name, None)
}

pub fn encode_delivery_announce_app_data(
    display_name: &str,
    stamp_cost: Option<u32>,
) -> Option<Vec<u8>> {
    encode_delivery_announce_app_data_with_capabilities(display_name, stamp_cost, &[])
}

pub fn encode_delivery_announce_app_data_with_capabilities(
    display_name: &str,
    stamp_cost: Option<u32>,
    capabilities: &[String],
) -> Option<Vec<u8>> {
    let normalized = normalize_display_name(display_name)?;
    let stamp_cost = stamp_cost
        .filter(|cost| *cost > 0 && *cost < 255)
        .map(rmpv::Value::from)
        .unwrap_or(rmpv::Value::Nil);
    let mut peer_data = vec![rmpv::Value::Binary(normalized.into_bytes()), stamp_cost];
    let capabilities = normalize_capabilities(capabilities);
    if !capabilities.is_empty() {
        let caps =
            rmpv::Value::Array(capabilities.into_iter().map(rmpv::Value::from).collect::<Vec<_>>());
        let payload = rmpv::Value::Map(vec![
            (rmpv::Value::from("app"), rmpv::Value::from("rch")),
            (rmpv::Value::from("schema"), rmpv::Value::from(1)),
            (rmpv::Value::from("caps"), caps),
        ]);
        peer_data.push(rmpv::Value::Binary(encode_msgpack(&payload, "delivery capabilities")?));
    }
    let peer_data = rmpv::Value::Array(peer_data);
    encode_msgpack(&peer_data, "delivery announce")
}

pub fn normalize_capabilities(capabilities: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for capability in capabilities {
        let capability = capability.trim().to_ascii_lowercase();
        if capability.is_empty()
            || capability.chars().any(|ch| {
                !(ch.is_ascii_lowercase()
                    || ch.is_ascii_digit()
                    || ch == '_'
                    || ch == '-'
                    || ch == '.')
            })
            || normalized.iter().any(|existing| existing == &capability)
        {
            continue;
        }
        normalized.push(capability);
    }
    normalized
}

#[derive(Debug, Clone, Copy)]
pub struct PropagationNodeAnnounceConfig {
    pub enabled: bool,
    pub timebase: i64,
    pub transfer_limit_kb: u32,
    pub sync_limit_kb: u32,
    pub stamp_cost: u32,
    pub stamp_cost_flexibility: u32,
    pub peering_cost: u32,
}

impl Default for PropagationNodeAnnounceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timebase: 0,
            transfer_limit_kb: 256,
            sync_limit_kb: 10240,
            stamp_cost: 16,
            stamp_cost_flexibility: 3,
            peering_cost: 18,
        }
    }
}

pub fn encode_propagation_node_app_data(
    display_name: Option<&str>,
    config: PropagationNodeAnnounceConfig,
) -> Option<Vec<u8>> {
    let mut metadata = Vec::new();
    if let Some(name) = display_name.and_then(normalize_display_name) {
        metadata.push((rmpv::Value::from(1_i64), rmpv::Value::Binary(name.into_bytes())));
    }
    let announce_data = rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::from(config.timebase),
        rmpv::Value::Boolean(config.enabled),
        rmpv::Value::from(config.transfer_limit_kb),
        rmpv::Value::from(config.sync_limit_kb),
        rmpv::Value::Array(vec![
            rmpv::Value::from(config.stamp_cost),
            rmpv::Value::from(config.stamp_cost_flexibility),
            rmpv::Value::from(config.peering_cost),
        ]),
        rmpv::Value::Map(metadata),
    ]);
    encode_msgpack(&announce_data, "propagation announce")
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

    let text = decode_utf8(app_data, "announce app_data utf8 fallback")?;
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

pub fn delivery_stamp_cost_from_app_data(data: &[u8]) -> Option<u32> {
    let decoded = decode_msgpack::<rmpv::Value>(data, "delivery stamp cost")?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => return None,
    };
    entries.get(1).and_then(rmp_value_to_u32).filter(|cost| (1..255).contains(cost))
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
        let decoded: rmpv::Value = decode_msgpack(data, "delivery display name")?;
        let entries = match decoded {
            rmpv::Value::Array(entries) => entries,
            _ => return None,
        };

        let first = entries.first()?;
        match first {
            rmpv::Value::Nil => None,
            rmpv::Value::Binary(bytes) => decode_utf8_owned(bytes.clone(), "delivery display name"),
            rmpv::Value::String(text) => text.as_str().map(|value| value.to_string()),
            _ => None,
        }
    } else {
        decode_utf8(data, "delivery display name fallback").map(|value| value.to_string())
    }
}

fn pn_name_from_app_data(data: &[u8]) -> Option<String> {
    const PN_META_NAME: u8 = 0x01;

    let decoded = decode_msgpack::<rmpv::Value>(data, "propagation metadata name")?;
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
            decode_utf8(candidate, "propagation metadata key").is_some_and(|candidate| {
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
        rmpv::Value::Binary(bytes) => decode_utf8_owned(bytes.clone(), "string-like msgpack value"),
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

    let decoded = decode_msgpack::<rmpv::Value>(data, "propagation announce costs")?;
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
        rmpv::Value::Binary(bytes) => decode_utf8_owned(bytes.clone(), "cost map key")
            .map(|key| key.trim().to_ascii_lowercase()),
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
            rmpv::Value::Binary(bytes) => parse_text_to_u32(decode_utf8(bytes, "cost value")?),
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

fn encode_msgpack(value: &rmpv::Value, context: &str) -> Option<Vec<u8>> {
    let encoded = rmp_serde::to_vec(value)
        .inspect_err(|err| log::warn!("[daemon] failed to encode {context}: {err}"));
    encoded.ok()
}

fn decode_msgpack<T>(data: &[u8], context: &str) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let decoded = rmp_serde::from_slice(data)
        .inspect_err(|err| log::warn!("[daemon] failed to decode {context}: {err}"));
    decoded.ok()
}

fn decode_utf8<'a>(data: &'a [u8], context: &str) -> Option<&'a str> {
    let text = std::str::from_utf8(data)
        .inspect_err(|err| log::warn!("[daemon] invalid UTF-8 in {context}: {err}"));
    text.ok()
}

fn decode_utf8_owned(data: Vec<u8>, context: &str) -> Option<String> {
    let text = String::from_utf8(data)
        .inspect_err(|err| log::warn!("[daemon] invalid UTF-8 in {context}: {err}"));
    text.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_delivery_stamp_cost_from_second_app_data_slot() {
        let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Peer Name".to_vec()),
            rmpv::Value::from(19),
        ]))
        .expect("encode app data");

        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()), Some(19));
        assert_eq!(pn_stamp_cost_from_app_data(app_data.as_slice()), None);
    }

    #[test]
    fn rejects_python_invalid_delivery_stamp_costs_from_second_app_data_slot() {
        for invalid_cost in [0, 255, 256] {
            let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
                rmpv::Value::Binary(b"Peer Name".to_vec()),
                rmpv::Value::from(invalid_cost),
            ]))
            .expect("encode app data");

            assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()), None);
        }
    }

    #[test]
    fn encodes_python_delivery_app_data_with_optional_stamp_cost() {
        let app_data =
            encode_delivery_announce_app_data("Peer Name", Some(21)).expect("encode app data");

        assert_eq!(
            parse_peer_name_from_app_data(app_data.as_slice()),
            Some(("Peer Name".to_string(), "delivery_app_data"))
        );
        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()), Some(21));

        let app_data_without_cost =
            encode_delivery_announce_app_data("Peer Name", Some(255)).expect("encode app data");
        assert_eq!(delivery_stamp_cost_from_app_data(app_data_without_cost.as_slice()), None);
    }

    #[test]
    fn encodes_delivery_app_data_with_rch_capability_payload() {
        let app_data = encode_delivery_announce_app_data_with_capabilities(
            "RCH Rust",
            Some(17),
            &["r3akt".to_string(), "topic_broker".to_string(), "telemetry_relay".to_string()],
        )
        .expect("encode app data");

        assert_eq!(
            parse_peer_name_from_app_data(app_data.as_slice()),
            Some(("RCH Rust".to_string(), "delivery_app_data"))
        );
        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()), Some(17));
        let rmpv::Value::Array(entries) =
            rmp_serde::from_slice::<rmpv::Value>(app_data.as_slice()).expect("decode app data")
        else {
            panic!("expected app data array");
        };
        assert_eq!(entries.len(), 3);
        let rmpv::Value::Binary(payload) = &entries[2] else {
            panic!("expected capability payload");
        };
        let rmpv::Value::Map(metadata) =
            rmp_serde::from_slice::<rmpv::Value>(payload).expect("decode capabilities")
        else {
            panic!("expected capability metadata");
        };
        assert!(metadata
            .iter()
            .any(|(key, value)| key.as_str() == Some("app") && value.as_str() == Some("rch")));
        assert!(metadata.iter().any(|(key, value)| {
            key.as_str() == Some("caps")
                && matches!(value, rmpv::Value::Array(items) if items.len() == 3)
        }));
    }

    #[test]
    fn encodes_python_propagation_app_data_with_configured_costs() {
        let app_data = encode_propagation_node_app_data(
            Some(" Peer Node "),
            PropagationNodeAnnounceConfig {
                timebase: 1_700_000_000,
                stamp_cost: 21,
                stamp_cost_flexibility: 5,
                peering_cost: 13,
                ..PropagationNodeAnnounceConfig::default()
            },
        )
        .expect("encode propagation app data");

        assert_eq!(
            parse_peer_name_from_app_data(app_data.as_slice()),
            Some(("Peer Node".to_string(), "pn_meta"))
        );
        assert_eq!(pn_stamp_cost_from_app_data(app_data.as_slice()), Some(21));
        assert_eq!(pn_stamp_cost_flexibility_from_app_data(app_data.as_slice()), Some(5));
        assert_eq!(pn_peering_cost_from_app_data(app_data.as_slice()), Some(13));
    }
}
