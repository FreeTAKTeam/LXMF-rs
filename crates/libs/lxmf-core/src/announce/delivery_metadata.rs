use alloc::string::{String, ToString};
use alloc::vec::Vec;

use rmpv::Value;

use super::{normalize_display_name, AnnounceDecodeError, AnnounceEncodeError};

const CAPABILITY_METADATA_APP: &str = "rch";
const CAPABILITY_METADATA_SCHEMA: i64 = 1;

pub fn normalize_announce_capabilities(capabilities: &[String]) -> Vec<String> {
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

pub fn encode_delivery_announce_app_data_with_capabilities(
    display_name: &str,
    stamp_cost: Option<u32>,
    capabilities: &[String],
) -> Result<Vec<u8>, AnnounceEncodeError> {
    let normalized =
        normalize_display_name(display_name).ok_or(AnnounceEncodeError::InvalidDisplayName)?;
    let stamp_cost =
        stamp_cost.filter(|cost| *cost > 0 && *cost < 255).map(Value::from).unwrap_or(Value::Nil);
    let mut peer_data = alloc::vec![Value::Binary(normalized.into_bytes()), stamp_cost];
    let capabilities = normalize_announce_capabilities(capabilities);
    if !capabilities.is_empty() {
        let caps = Value::Array(capabilities.into_iter().map(Value::from).collect());
        let metadata = Value::Map(alloc::vec![
            (Value::from("app"), Value::from(CAPABILITY_METADATA_APP)),
            (Value::from("schema"), Value::from(CAPABILITY_METADATA_SCHEMA)),
            (Value::from("caps"), caps),
        ]);
        let encoded_metadata = rmp_serde::to_vec(&metadata)?;
        peer_data.push(Value::Binary(encoded_metadata));
    }
    rmp_serde::to_vec(&Value::Array(peer_data)).map_err(AnnounceEncodeError::from)
}

pub fn capabilities_from_delivery_app_data(
    data: &[u8],
) -> Result<Vec<String>, AnnounceDecodeError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let decoded: Value = rmp_serde::from_slice(data)?;
    let Value::Array(entries) = decoded else {
        return Ok(Vec::new());
    };
    // Slot one is the standard stamp cost, but earlier REM builds placed their
    // capability map there. An integer/nil stamp never matches this decoder,
    // so scanning extensions from slot one preserves that deployed layout.
    for entry in entries.iter().skip(1) {
        if let Some(capabilities) = capabilities_from_metadata_value(entry)? {
            return Ok(capabilities);
        }
    }
    Ok(Vec::new())
}

fn capabilities_from_metadata_value(
    value: &Value,
) -> Result<Option<Vec<String>>, AnnounceDecodeError> {
    match value {
        Value::Map(entries) => Ok(entries.iter().find_map(|(key, value)| {
            matches!(key, Value::String(actual) if matches!(actual.as_str(), Some("caps" | "announce_capabilities")))
                .then(|| capabilities_from_array(value))
        })),
        Value::Binary(bytes) => {
            let nested = rmp_serde::from_slice::<Value>(bytes)?;
            capabilities_from_metadata_value(&nested)
        }
        _ => Ok(None),
    }
}

fn capabilities_from_array(value: &Value) -> Vec<String> {
    let Value::Array(values) = value else {
        return Vec::new();
    };
    normalize_announce_capabilities(
        &values
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::announce::{display_name_from_app_data, stamp_cost_from_app_data};

    #[test]
    fn structured_capabilities_preserve_standard_delivery_slots() {
        let app_data = encode_delivery_announce_app_data_with_capabilities(
            "Field Team One",
            Some(17),
            &[
                "R3AKT".to_string(),
                "emergencymessages".to_string(),
                "rem.standard_lxmf_receipts.v1".to_string(),
            ],
        )
        .expect("structured delivery announce");

        assert_eq!(
            display_name_from_app_data(Some(app_data.as_slice())),
            Some("Field Team One".to_string())
        );
        assert_eq!(stamp_cost_from_app_data(Some(app_data.as_slice())), Some(17));
        assert_eq!(
            capabilities_from_delivery_app_data(app_data.as_slice()).expect("capability metadata"),
            alloc::vec![
                "r3akt".to_string(),
                "emergencymessages".to_string(),
                "rem.standard_lxmf_receipts.v1".to_string(),
            ]
        );
    }

    #[test]
    fn delivery_announce_without_capabilities_keeps_python_shape() {
        let app_data =
            encode_delivery_announce_app_data_with_capabilities("Field Team Two", None, &[])
                .expect("standard delivery announce");
        let decoded: Value = rmp_serde::from_slice(app_data.as_slice()).expect("msgpack");
        assert!(matches!(decoded, Value::Array(entries) if entries.len() == 2));
        assert!(capabilities_from_delivery_app_data(app_data.as_slice())
            .expect("empty capabilities")
            .is_empty());
    }

    #[test]
    fn decoder_accepts_legacy_rem_capabilities_in_stamp_slot() {
        let legacy = Value::Array(alloc::vec![
            Value::from("Legacy REM"),
            Value::Map(alloc::vec![(
                Value::from("caps"),
                Value::Array(alloc::vec![Value::from("R3AKT"), Value::from("Telemetry")]),
            )]),
        ]);
        let app_data = rmp_serde::to_vec(&legacy).expect("legacy msgpack");

        assert_eq!(
            capabilities_from_delivery_app_data(app_data.as_slice()).expect("legacy capabilities"),
            alloc::vec!["r3akt".to_string(), "telemetry".to_string()]
        );
    }

    #[test]
    fn decoder_rejects_malformed_binary_capability_metadata() {
        let malformed = Value::Array(alloc::vec![
            Value::from("Malformed metadata"),
            Value::Nil,
            Value::Binary(alloc::vec![0xc1]),
        ]);
        let app_data = rmp_serde::to_vec(&malformed).expect("delivery announce msgpack");

        assert!(matches!(
            capabilities_from_delivery_app_data(app_data.as_slice()),
            Err(AnnounceDecodeError::Msgpack(_))
        ));
    }
}
