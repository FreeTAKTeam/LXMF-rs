pub use lxmf::announce::AnnounceEncodeError;

use crate::text::{decode_utf8, decode_utf8_owned};
use core::fmt;

#[derive(Debug)]
pub enum AnnounceNamesDecodeError {
    Msgpack(rmp_serde::decode::Error),
    Utf8(std::string::FromUtf8Error),
    Malformed(&'static str),
}

impl fmt::Display for AnnounceNamesDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Msgpack(err) => write!(f, "msgpack decode error: {err}"),
            Self::Utf8(err) => write!(f, "invalid UTF-8: {err}"),
            Self::Malformed(msg) => write!(f, "malformed announce data: {msg}"),
        }
    }
}

impl std::error::Error for AnnounceNamesDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Msgpack(err) => Some(err),
            Self::Utf8(err) => Some(err),
            Self::Malformed(_) => None,
        }
    }
}

impl From<rmp_serde::decode::Error> for AnnounceNamesDecodeError {
    fn from(err: rmp_serde::decode::Error) -> Self {
        Self::Msgpack(err)
    }
}

impl From<std::string::FromUtf8Error> for AnnounceNamesDecodeError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Self::Utf8(err)
    }
}

pub fn encode_delivery_display_name_app_data(
    display_name: &str,
) -> Result<Vec<u8>, AnnounceEncodeError> {
    encode_delivery_announce_app_data(display_name, None)
}

pub fn encode_delivery_announce_app_data(
    display_name: &str,
    stamp_cost: Option<u32>,
) -> Result<Vec<u8>, AnnounceEncodeError> {
    encode_delivery_announce_app_data_with_capabilities(display_name, stamp_cost, &[])
}

pub fn encode_delivery_announce_app_data_with_capabilities(
    display_name: &str,
    stamp_cost: Option<u32>,
    capabilities: &[String],
) -> Result<Vec<u8>, AnnounceEncodeError> {
    let normalized =
        normalize_display_name(display_name).ok_or(AnnounceEncodeError::InvalidDisplayName)?;
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
    Ok(encode_msgpack(&peer_data, "delivery announce")?)
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
) -> Result<Vec<u8>, rmp_serde::encode::Error> {
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

/// S: `Ok(None)` = no name present; `Err` = parse failure.
/// Falls through delivery → PN → raw-UTF-8, catching `Err` from each R helper
/// rather than propagating, so that all three paths are attempted.
pub fn parse_peer_name_from_app_data(
    app_data: &[u8],
) -> Result<Option<(String, &'static str)>, AnnounceNamesDecodeError> {
    if app_data.is_empty() {
        return Err(AnnounceNamesDecodeError::Malformed("empty announce app_data"));
    }

    if is_msgpack_array_prefix(app_data[0]) {
        if let Ok(name) = display_name_from_app_data(app_data) {
            if let Some(name) = normalize_display_name(&name) {
                return Ok(Some((name, "delivery_app_data")));
            }
        }
    }

    if let Ok(name) = pn_name_from_app_data(app_data) {
        if let Some(name) = normalize_display_name(&name) {
            return Ok(Some((name, "pn_meta")));
        }
    }

    let text = decode_utf8(app_data, "announce app_data utf8 fallback")
        .map_err(|_| AnnounceNamesDecodeError::Malformed("non-UTF-8 raw announce app_data"))?;
    Ok(normalize_display_name(text).map(|name| (name, "app_data_utf8")))
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

/// R: `Ok(u32)` = stamp cost present; `Err` = decode failure or cost absent.
pub fn pn_stamp_cost_from_app_data(data: &[u8]) -> Result<u32, AnnounceNamesDecodeError> {
    parse_announce_cost_from_app_data(data, 0)?
        .ok_or(AnnounceNamesDecodeError::Malformed("PN stamp cost not present in announce data"))
}

/// S: `Ok(Some(u32))` = cost present; `Ok(None)` = cost field absent or Nil; `Err` = decode failure.
pub fn delivery_stamp_cost_from_app_data(
    data: &[u8],
) -> Result<Option<u32>, AnnounceNamesDecodeError> {
    let decoded = decode_msgpack::<rmpv::Value>(data, "delivery stamp cost")?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => {
            return Err(AnnounceNamesDecodeError::Malformed(
                "delivery stamp cost data is not an array",
            ))
        }
    };
    let Some(cost_value) = entries.get(1) else {
        return Err(AnnounceNamesDecodeError::Malformed(
            "no stamp cost slot in delivery app_data array",
        ));
    };
    // Nil is the explicit "no cost" marker in the delivery format.
    if matches!(cost_value, rmpv::Value::Nil) {
        return Ok(None);
    }
    let cost = rmp_value_to_u32(cost_value)?;
    Ok((1..255).contains(&cost).then_some(cost))
}

/// R: `Ok(u32)` = flexibility present; `Err` = decode failure or field absent.
pub fn pn_stamp_cost_flexibility_from_app_data(
    data: &[u8],
) -> Result<u32, AnnounceNamesDecodeError> {
    parse_announce_cost_from_app_data(data, 1)?.ok_or(AnnounceNamesDecodeError::Malformed(
        "PN stamp cost flexibility not present in announce data",
    ))
}

/// R: `Ok(u32)` = peering cost present; `Err` = decode failure or field absent.
pub fn pn_peering_cost_from_app_data(data: &[u8]) -> Result<u32, AnnounceNamesDecodeError> {
    parse_announce_cost_from_app_data(data, 2)?
        .ok_or(AnnounceNamesDecodeError::Malformed("PN peering cost not present in announce data"))
}

fn is_msgpack_array_prefix(byte: u8) -> bool {
    (0x90..=0x9f).contains(&byte) || byte == 0xdc || byte == 0xdd
}

/// R: `Ok(String)` = name decoded; `Err` = any failure (decode, wrong type, Nil, etc.).
fn display_name_from_app_data(data: &[u8]) -> Result<String, AnnounceNamesDecodeError> {
    if data.is_empty() {
        return Err(AnnounceNamesDecodeError::Malformed("empty delivery display name data"));
    }

    let decoded: rmpv::Value = decode_msgpack(data, "delivery display name")?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => {
            return Err(AnnounceNamesDecodeError::Malformed(
                "delivery display name is not an array",
            ))
        }
    };

    let Some(first) = entries.first() else {
        return Err(AnnounceNamesDecodeError::Malformed(
            "no first element in delivery display name array",
        ));
    };
    match first {
        rmpv::Value::Nil => Err(AnnounceNamesDecodeError::Malformed("nil delivery display name")),
        rmpv::Value::Binary(bytes) => decode_utf8_owned(bytes.clone(), "delivery display name")
            .map_err(AnnounceNamesDecodeError::Utf8),
        rmpv::Value::String(text) => text
            .as_str()
            .map(|value| value.to_string())
            .ok_or(AnnounceNamesDecodeError::Malformed("non-UTF-8 delivery display name string")),
        _ => Err(AnnounceNamesDecodeError::Malformed("unexpected type for delivery display name")),
    }
}

/// R: `Ok(String)` = PN name decoded; `Err` = any failure (decode, missing field, wrong type, etc.).
fn pn_name_from_app_data(data: &[u8]) -> Result<String, AnnounceNamesDecodeError> {
    const PN_META_NAME: u8 = 0x01;

    let decoded = decode_msgpack::<rmpv::Value>(data, "propagation metadata name")?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => return Err(AnnounceNamesDecodeError::Malformed("PN metadata is not an array")),
    };

    let Some(metadata) = entries.get(6) else {
        return Err(AnnounceNamesDecodeError::Malformed("no metadata element in PN announce"));
    };
    let rmpv::Value::Map(entries) = metadata else {
        return Err(AnnounceNamesDecodeError::Malformed("PN metadata element is not a map"));
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

    Err(AnnounceNamesDecodeError::Malformed("name key not found in PN metadata"))
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
            decode_utf8(candidate, "propagation metadata key").is_ok_and(|candidate| {
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

/// R: `Ok(String)` = convertible; `Err` = unsupported type or non-UTF-8.
fn string_like_value_to_string(value: &rmpv::Value) -> Result<String, AnnounceNamesDecodeError> {
    match value {
        rmpv::Value::Binary(bytes) => decode_utf8_owned(bytes.clone(), "string-like msgpack value")
            .map_err(AnnounceNamesDecodeError::Utf8),
        rmpv::Value::String(text) => text
            .as_str()
            .map(|s| s.to_string())
            .ok_or(AnnounceNamesDecodeError::Malformed("non-UTF-8 msgpack string")),
        rmpv::Value::Integer(value) => value
            .as_i64()
            .map(|v| v.to_string())
            .ok_or(AnnounceNamesDecodeError::Malformed("integer value out of i64 range")),
        rmpv::Value::F64(value) => {
            if value.fract() == 0.0 {
                Ok(format!("{value:.0}"))
            } else {
                Err(AnnounceNamesDecodeError::Malformed(
                    "f64 with fractional part not usable as string name",
                ))
            }
        }
        rmpv::Value::F32(value) => {
            let value = f64::from(*value);
            if value.fract() == 0.0 {
                Ok(format!("{value:.0}"))
            } else {
                Err(AnnounceNamesDecodeError::Malformed(
                    "f32 with fractional part not usable as string name",
                ))
            }
        }
        _ => Err(AnnounceNamesDecodeError::Malformed("unsupported type for string-like value")),
    }
}

/// S: `Ok(Some(u32))` = cost present; `Ok(None)` = cost entry absent; `Err` = decode or coercion failure.
fn parse_announce_cost_from_app_data(
    data: &[u8],
    index: usize,
) -> Result<Option<u32>, AnnounceNamesDecodeError> {
    if index > 2 {
        return Err(AnnounceNamesDecodeError::Malformed("cost index out of range"));
    }

    let decoded = decode_msgpack::<rmpv::Value>(data, "propagation announce costs")?;
    let entries = match decoded {
        rmpv::Value::Array(entries) => entries,
        _ => return Err(AnnounceNamesDecodeError::Malformed("PN announce data is not an array")),
    };

    let Some(cost_entry) = entries.get(5) else {
        return Err(AnnounceNamesDecodeError::Malformed("no costs element in PN announce data"));
    };

    match cost_entry {
        rmpv::Value::Array(costs) => costs.get(index).map(rmp_value_to_u32).transpose(),
        rmpv::Value::Map(costs) => parse_announce_cost_from_map(costs, index),
        _ => Err(AnnounceNamesDecodeError::Malformed("unexpected type for PN announce costs")),
    }
}

fn parse_announce_cost_from_map(
    costs: &[(rmpv::Value, rmpv::Value)],
    index: usize,
) -> Result<Option<u32>, AnnounceNamesDecodeError> {
    let target_key = match index {
        0 => ["stamp_cost", "0"],
        1 => ["stamp_cost_flexibility", "1"],
        2 => ["peering_cost", "2"],
        _ => return Err(AnnounceNamesDecodeError::Malformed("cost index out of range")),
    };

    for (key, value) in costs {
        let Ok(cost_key) = cost_map_key_text(key) else {
            continue;
        };
        if target_key.contains(&cost_key.as_str()) {
            return rmp_value_to_u32(value).map(Some);
        }
    }
    Ok(None)
}

/// R: `Ok(String)` = key text extracted; `Err` = unsupported key type.
fn cost_map_key_text(key: &rmpv::Value) -> Result<String, AnnounceNamesDecodeError> {
    match key {
        rmpv::Value::String(text) => Ok(text
            .as_str()
            .ok_or(AnnounceNamesDecodeError::Malformed("non-UTF-8 cost map key"))?
            .trim()
            .to_ascii_lowercase()),
        rmpv::Value::Binary(bytes) => Ok(decode_utf8_owned(bytes.clone(), "cost map key")
            .map_err(AnnounceNamesDecodeError::Utf8)?
            .trim()
            .to_ascii_lowercase()),
        rmpv::Value::Integer(value) => value
            .as_u64()
            .map(|key| key.to_string())
            .or_else(|| value.as_i64().map(|key| key.to_string()))
            .ok_or(AnnounceNamesDecodeError::Malformed(
                "integer cost map key out of i64/u64 range",
            )),
        _ => Err(AnnounceNamesDecodeError::Malformed("unsupported type for cost map key")),
    }
}

/// R: `Ok(u32)` = coercion succeeded; `Err` = type not representable as u32.
fn rmp_value_to_u32(value: &rmpv::Value) -> Result<u32, AnnounceNamesDecodeError> {
    if let Some(u) = value.as_u64().and_then(|v| u32::try_from(v).ok()) {
        return Ok(u);
    }
    if let Some(u) = value.as_i64().and_then(|v| u32::try_from(v).ok()) {
        return Ok(u);
    }
    match value {
        rmpv::Value::F64(v) => parse_f64_to_u32(*v),
        rmpv::Value::F32(v) => parse_f64_to_u32(f64::from(*v)),
        rmpv::Value::Boolean(v) => Ok(u32::from(*v)),
        rmpv::Value::Binary(bytes) => {
            let text = decode_utf8(bytes, "cost value")
                .map_err(|_| AnnounceNamesDecodeError::Malformed("non-UTF-8 binary cost value"))?;
            parse_text_to_u32(text)
        }
        rmpv::Value::String(text) => {
            parse_text_to_u32(text.as_str().ok_or(AnnounceNamesDecodeError::Malformed(
                "non-UTF-8 msgpack string cost value",
            ))?)
        }
        _ => Err(AnnounceNamesDecodeError::Malformed("unsupported type for u32 cost value")),
    }
}

fn parse_f64_to_u32(value: f64) -> Result<u32, AnnounceNamesDecodeError> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > u32::MAX as f64 {
        return Err(AnnounceNamesDecodeError::Malformed(
            "f64 value out of range or non-integer for u32",
        ));
    }
    Ok(value as u32)
}

fn parse_text_to_u32(value: &str) -> Result<u32, AnnounceNamesDecodeError> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| AnnounceNamesDecodeError::Malformed("failed to parse text as u32"))
}

fn encode_msgpack(value: &rmpv::Value, context: &str) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(value)
        .inspect_err(|err| log::warn!("[daemon] failed to encode {context}: {err}"))
}

fn decode_msgpack<T>(data: &[u8], context: &str) -> Result<T, AnnounceNamesDecodeError>
where
    T: serde::de::DeserializeOwned,
{
    rmp_serde::from_slice(data)
        .inspect_err(|err| log::warn!("[daemon] failed to decode {context}: {err}"))
        .map_err(AnnounceNamesDecodeError::Msgpack)
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

        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), Some(19));
        assert!(pn_stamp_cost_from_app_data(app_data.as_slice()).is_err());
    }

    #[test]
    fn rejects_python_invalid_delivery_stamp_costs_from_second_app_data_slot() {
        for invalid_cost in [0, 255, 256] {
            let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
                rmpv::Value::Binary(b"Peer Name".to_vec()),
                rmpv::Value::from(invalid_cost),
            ]))
            .expect("encode app data");

            assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), None);
        }
    }

    #[test]
    fn encodes_python_delivery_app_data_with_optional_stamp_cost() {
        let app_data =
            encode_delivery_announce_app_data("Peer Name", Some(21)).expect("encode app data");

        assert_eq!(
            parse_peer_name_from_app_data(app_data.as_slice()).expect("ok"),
            Some(("Peer Name".to_string(), "delivery_app_data"))
        );
        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), Some(21));

        let app_data_without_cost =
            encode_delivery_announce_app_data("Peer Name", Some(255)).expect("encode app data");
        assert_eq!(
            delivery_stamp_cost_from_app_data(app_data_without_cost.as_slice()).expect("ok"),
            None
        );
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
            parse_peer_name_from_app_data(app_data.as_slice()).expect("ok"),
            Some(("RCH Rust".to_string(), "delivery_app_data"))
        );
        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), Some(17));
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
            parse_peer_name_from_app_data(app_data.as_slice()).expect("ok"),
            Some(("Peer Node".to_string(), "pn_meta"))
        );
        assert_eq!(pn_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), 21);
        assert_eq!(pn_stamp_cost_flexibility_from_app_data(app_data.as_slice()).expect("ok"), 5);
        assert_eq!(pn_peering_cost_from_app_data(app_data.as_slice()).expect("ok"), 13);
    }
}
