use alloc::string::{FromUtf8Error, String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::constants::{PN_META_NAME, SF_COMPRESSION};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceSlot {
    pub id: u8,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceParseError(&'static str);

#[derive(Debug)]
pub enum AnnounceEncodeError {
    InvalidDisplayName,
    Encode(rmp_serde::encode::Error),
}

impl fmt::Display for AnnounceEncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDisplayName => {
                f.write_str("display name is empty or contains control characters")
            }
            Self::Encode(err) => write!(f, "msgpack encode error: {err}"),
        }
    }
}

impl From<rmp_serde::encode::Error> for AnnounceEncodeError {
    fn from(err: rmp_serde::encode::Error) -> Self {
        Self::Encode(err)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for AnnounceEncodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(err) => Some(err),
            Self::InvalidDisplayName => None,
        }
    }
}

#[derive(Debug)]
pub enum AnnounceDecodeError {
    Msgpack(rmp_serde::decode::Error),
    Utf8(FromUtf8Error),
}

impl fmt::Display for AnnounceDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Msgpack(err) => write!(f, "msgpack decode error: {err}"),
            Self::Utf8(err) => write!(f, "invalid UTF-8: {err}"),
        }
    }
}

impl From<rmp_serde::decode::Error> for AnnounceDecodeError {
    fn from(err: rmp_serde::decode::Error) -> Self {
        Self::Msgpack(err)
    }
}

impl From<FromUtf8Error> for AnnounceDecodeError {
    fn from(err: FromUtf8Error) -> Self {
        Self::Utf8(err)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for AnnounceDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Msgpack(err) => Some(err),
            Self::Utf8(err) => Some(err),
        }
    }
}

impl fmt::Display for AnnounceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PnAnnounceParseError {
    InvalidMsgpack,
    NotArray,
    InsufficientPeerData,
    InvalidTimebase,
    IndeterminatePropagationNodeStatus,
    InvalidTransferOrSyncLimit,
    InvalidStampCosts,
    InvalidStampCostValues,
    InvalidMetadata,
}

impl fmt::Display for PnAnnounceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMsgpack => f.write_str("invalid propagation node announce msgpack"),
            Self::NotArray => f.write_str("propagation node announce data must be an array"),
            Self::InsufficientPeerData => f.write_str(
                "invalid announce data: insufficient peer data, likely from deprecated LXMF version",
            ),
            Self::InvalidTimebase => {
                f.write_str("invalid announce data: could not decode timebase")
            }
            Self::IndeterminatePropagationNodeStatus => f.write_str(
                "invalid announce data: indeterminate propagation node status",
            ),
            Self::InvalidTransferOrSyncLimit => f.write_str(
                "invalid announce data: could not decode propagation transfer or sync limit",
            ),
            Self::InvalidStampCosts => {
                f.write_str("invalid announce data: could not decode stamp costs")
            }
            Self::InvalidStampCostValues => f.write_str(
                "invalid announce data: could not decode target, flexibility, or peering stamp cost",
            ),
            Self::InvalidMetadata => {
                f.write_str("invalid announce data: could not decode metadata")
            }
        }
    }
}

pub fn parse_announce_slots(data: &[u8]) -> Result<Vec<AnnounceSlot>, AnnounceParseError> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        if i + 2 > data.len() {
            return Err(AnnounceParseError("truncated announce slot header"));
        }
        let id = data[i];
        let len = data[i + 1] as usize;
        i += 2;
        if i + len > data.len() {
            return Err(AnnounceParseError("announce slot length exceeds payload"));
        }
        out.push(AnnounceSlot { id, value: data[i..i + len].to_vec() });
        i += len;
    }
    Ok(out)
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

pub fn encode_delivery_display_name_app_data(
    display_name: &str,
) -> Result<Vec<u8>, AnnounceEncodeError> {
    let normalized =
        normalize_display_name(display_name).ok_or(AnnounceEncodeError::InvalidDisplayName)?;
    let peer_data =
        rmpv::Value::Array(vec![rmpv::Value::Binary(normalized.into_bytes()), rmpv::Value::Nil]);
    Ok(encode_msgpack(&peer_data)?)
}

pub fn display_name_from_delivery_app_data(
    data: &[u8],
) -> Result<Option<String>, AnnounceDecodeError> {
    if data.is_empty() {
        return Ok(None);
    }

    let decoded: rmpv::Value = decode_msgpack(data)?;
    let name = match decoded {
        rmpv::Value::Array(values) => match values.into_iter().next() {
            Some(rmpv::Value::Binary(bytes)) => {
                normalize_display_name(decode_utf8_owned(bytes)?.as_str())
            }
            Some(rmpv::Value::String(value)) => {
                normalize_display_name(decode_utf8_owned(value.into_bytes())?.as_str())
            }
            _ => None,
        },
        rmpv::Value::Binary(bytes) => normalize_display_name(decode_utf8_owned(bytes)?.as_str()),
        rmpv::Value::String(value) => {
            normalize_display_name(decode_utf8_owned(value.into_bytes())?.as_str())
        }
        _ => None,
    };
    Ok(name)
}

pub fn display_name_from_app_data(app_data: Option<&[u8]>) -> Option<String> {
    let data = non_empty_app_data(app_data)?;
    if app_data_uses_current_format(data) {
        let peer_data: rmpv::Value = decode_msgpack(data).ok()?;
        let rmpv::Value::Array(values) = peer_data else {
            return None;
        };
        let display_name = values.first()?;
        if matches!(display_name, rmpv::Value::Nil) {
            return None;
        }
        value_to_utf8(display_name)
    } else {
        decode_utf8_owned(data.to_vec()).ok()
    }
}

pub fn stamp_cost_from_app_data(app_data: Option<&[u8]>) -> Option<i64> {
    let data = non_empty_app_data(app_data)?;
    if !app_data_uses_current_format(data) {
        return None;
    }
    let peer_data: rmpv::Value = decode_msgpack(data).ok()?;
    let rmpv::Value::Array(values) = peer_data else {
        return None;
    };
    values.get(1).and_then(value_to_i64)
}

pub fn compression_support_from_app_data(app_data: Option<&[u8]>) -> Option<bool> {
    let data = non_empty_app_data(app_data)?;
    if !app_data_uses_current_format(data) {
        return Some(true);
    }
    let peer_data: rmpv::Value = decode_msgpack(data).ok()?;
    let rmpv::Value::Array(values) = peer_data else {
        return None;
    };
    let Some(supported_features) = values.get(2) else {
        return Some(true);
    };
    let rmpv::Value::Array(features) = supported_features else {
        return Some(true);
    };
    Some(features.iter().any(|feature| value_to_i64(feature) == Some(i64::from(SF_COMPRESSION))))
}

pub fn pn_name_from_app_data(app_data: Option<&[u8]>) -> Option<String> {
    let data = app_data?;
    if !pn_announce_data_is_valid(data) {
        return None;
    }
    let rmpv::Value::Array(values) = decode_msgpack::<rmpv::Value>(data).ok()? else {
        return None;
    };
    let rmpv::Value::Map(metadata) = values.get(6)? else {
        return None;
    };
    metadata_value(metadata, PN_META_NAME).and_then(value_to_utf8)
}

pub fn pn_stamp_cost_from_app_data(app_data: Option<&[u8]>) -> Option<i64> {
    let data = app_data?;
    if !pn_announce_data_is_valid(data) {
        return None;
    }
    let rmpv::Value::Array(values) = decode_msgpack::<rmpv::Value>(data).ok()? else {
        return None;
    };
    let rmpv::Value::Array(stamp_costs) = values.get(5)? else {
        return None;
    };
    stamp_costs.first().and_then(value_to_i64)
}

pub fn pn_announce_data_is_valid(data: &[u8]) -> bool {
    validate_pn_announce_data(data).is_ok()
}

pub fn validate_pn_announce_data(data: &[u8]) -> Result<(), PnAnnounceParseError> {
    let decoded = rmp_serde::from_slice::<rmpv::Value>(data)
        .map_err(|_| PnAnnounceParseError::InvalidMsgpack)?;
    let rmpv::Value::Array(values) = decoded else {
        return Err(PnAnnounceParseError::NotArray);
    };
    if values.len() < 7 {
        return Err(PnAnnounceParseError::InsufficientPeerData);
    }
    if value_to_i64(&values[1]).is_none() {
        return Err(PnAnnounceParseError::InvalidTimebase);
    }
    if !matches!(values[2], rmpv::Value::Boolean(_)) {
        return Err(PnAnnounceParseError::IndeterminatePropagationNodeStatus);
    }
    if value_to_i64(&values[3]).is_none() || value_to_i64(&values[4]).is_none() {
        return Err(PnAnnounceParseError::InvalidTransferOrSyncLimit);
    }
    let rmpv::Value::Array(stamp_costs) = &values[5] else {
        return Err(PnAnnounceParseError::InvalidStampCosts);
    };
    if stamp_costs.len() < 3
        || stamp_costs.iter().take(3).any(|value| value_to_i64(value).is_none())
    {
        return Err(PnAnnounceParseError::InvalidStampCostValues);
    }
    if !matches!(values[6], rmpv::Value::Map(_)) {
        return Err(PnAnnounceParseError::InvalidMetadata);
    }
    Ok(())
}

fn non_empty_app_data(app_data: Option<&[u8]>) -> Option<&[u8]> {
    let data = app_data?;
    if data.is_empty() {
        None
    } else {
        Some(data)
    }
}

fn app_data_uses_current_format(data: &[u8]) -> bool {
    matches!(data.first(), Some(0x90..=0x9f) | Some(0xdc))
}

fn value_to_utf8(value: &rmpv::Value) -> Option<String> {
    match value {
        rmpv::Value::Binary(bytes) => decode_utf8_owned(bytes.clone()).ok(),
        rmpv::Value::String(text) => text.as_str().map(ToString::to_string),
        _ => None,
    }
}

fn value_to_i64(value: &rmpv::Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

fn metadata_value(metadata: &[(rmpv::Value, rmpv::Value)], key: u8) -> Option<&rmpv::Value> {
    metadata.iter().find_map(|(candidate, value)| {
        (value_to_i64(candidate) == Some(i64::from(key))).then_some(value)
    })
}

fn encode_msgpack(value: &rmpv::Value) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(value)
}

fn decode_msgpack<T>(data: &[u8]) -> Result<T, rmp_serde::decode::Error>
where
    T: serde::de::DeserializeOwned,
{
    rmp_serde::from_slice(data)
}

fn decode_utf8_owned(data: Vec<u8>) -> Result<String, FromUtf8Error> {
    String::from_utf8(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{
        PN_META_AUTH_BAND, PN_META_CUSTOM, PN_META_NAME, PN_META_SYNC_STRATUM,
        PN_META_SYNC_THROTTLE, PN_META_UTIL_PRESSURE, PN_META_VERSION, SF_COMPRESSION,
    };

    fn encode_value(value: &rmpv::Value) -> Vec<u8> {
        rmp_serde::to_vec(value).expect("test fixture encodes")
    }

    #[test]
    fn encode_and_decode_delivery_display_name_round_trip() {
        let encoded = encode_delivery_display_name_app_data("Alice Router").expect("encoded");
        let decoded = display_name_from_delivery_app_data(encoded.as_slice())
            .expect("decoded")
            .expect("name");
        assert_eq!(decoded, "Alice Router");
    }

    #[test]
    fn normalize_display_name_rejects_control_bytes() {
        assert!(normalize_display_name("Alice\nRouter").is_none());
    }

    #[test]
    fn display_name_from_invalid_utf8_string_surfaces_error() {
        // A msgpack `str` (fixstr len 2, 0xa2) carrying invalid UTF-8 bytes must surface a
        // decode error, not collapse into Ok(None) (which a caller can't distinguish from a
        // genuine absent name).
        let data = [0xa2_u8, 0xff, 0xfe];
        assert!(display_name_from_delivery_app_data(&data).is_err());
    }

    #[test]
    fn lxmf_module_display_name_helper_accepts_current_and_legacy_formats() {
        let current = encode_value(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b" Alice\0 Router ".to_vec()),
            rmpv::Value::Integer(12_i64.into()),
        ]));
        assert_eq!(
            display_name_from_app_data(Some(&current)),
            Some(" Alice\0 Router ".to_string())
        );
        assert_eq!(
            display_name_from_app_data(Some(b"Legacy Router")),
            Some("Legacy Router".to_string())
        );
        assert_eq!(display_name_from_app_data(Some(&[])), None);
        assert_eq!(display_name_from_app_data(None), None);
    }

    #[test]
    fn lxmf_module_stamp_cost_and_compression_helpers_match_python_defaults() {
        let with_compression = encode_value(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Alice".to_vec()),
            rmpv::Value::Integer(8_i64.into()),
            rmpv::Value::Array(vec![rmpv::Value::Integer(i64::from(SF_COMPRESSION).into())]),
        ]));
        let without_feature_list = encode_value(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Alice".to_vec()),
            rmpv::Value::Integer(4_i64.into()),
            rmpv::Value::String("not-a-list".into()),
        ]));
        let no_compression = encode_value(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Alice".to_vec()),
            rmpv::Value::Integer(2_i64.into()),
            rmpv::Value::Array(vec![rmpv::Value::Integer(0xAA_i64.into())]),
        ]));

        assert_eq!(stamp_cost_from_app_data(Some(&with_compression)), Some(8));
        assert_eq!(stamp_cost_from_app_data(Some(b"legacy")), None);
        assert_eq!(compression_support_from_app_data(Some(&with_compression)), Some(true));
        assert_eq!(compression_support_from_app_data(Some(&without_feature_list)), Some(true));
        assert_eq!(compression_support_from_app_data(Some(&no_compression)), Some(false));
        assert_eq!(compression_support_from_app_data(Some(b"legacy")), Some(true));
        assert_eq!(compression_support_from_app_data(None), None);
    }

    #[test]
    fn propagation_node_app_data_helpers_validate_metadata_shape() {
        let valid = encode_value(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Peer".to_vec()),
            rmpv::Value::Integer(17_i64.into()),
            rmpv::Value::Boolean(true),
            rmpv::Value::Integer(65_536_i64.into()),
            rmpv::Value::Integer(32_768_i64.into()),
            rmpv::Value::Array(vec![
                rmpv::Value::Integer(11_i64.into()),
                rmpv::Value::Integer(2_i64.into()),
                rmpv::Value::Integer(5_i64.into()),
            ]),
            rmpv::Value::Map(vec![
                (
                    rmpv::Value::Integer(i64::from(PN_META_VERSION).into()),
                    rmpv::Value::Integer(1_i64.into()),
                ),
                (
                    rmpv::Value::Integer(i64::from(PN_META_NAME).into()),
                    rmpv::Value::Binary(b"Node Alpha".to_vec()),
                ),
                (
                    rmpv::Value::Integer(i64::from(PN_META_SYNC_STRATUM).into()),
                    rmpv::Value::Integer(0_i64.into()),
                ),
                (
                    rmpv::Value::Integer(i64::from(PN_META_SYNC_THROTTLE).into()),
                    rmpv::Value::Integer(1_i64.into()),
                ),
                (
                    rmpv::Value::Integer(i64::from(PN_META_AUTH_BAND).into()),
                    rmpv::Value::Integer(0_i64.into()),
                ),
                (
                    rmpv::Value::Integer(i64::from(PN_META_UTIL_PRESSURE).into()),
                    rmpv::Value::Integer(0_i64.into()),
                ),
                (
                    rmpv::Value::Integer(i64::from(PN_META_CUSTOM).into()),
                    rmpv::Value::Map(Vec::new()),
                ),
            ]),
        ]));
        let invalid = encode_value(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Peer".to_vec()),
            rmpv::Value::String("bad-timebase".into()),
            rmpv::Value::Boolean(true),
            rmpv::Value::Integer(65_536_i64.into()),
            rmpv::Value::Integer(32_768_i64.into()),
            rmpv::Value::Array(vec![
                rmpv::Value::Integer(11_i64.into()),
                rmpv::Value::Integer(2_i64.into()),
                rmpv::Value::Integer(5_i64.into()),
            ]),
            rmpv::Value::Map(Vec::new()),
        ]));

        assert!(pn_announce_data_is_valid(&valid));
        assert_eq!(validate_pn_announce_data(&valid), Ok(()));
        assert_eq!(pn_name_from_app_data(Some(&valid)), Some("Node Alpha".to_string()));
        assert_eq!(pn_stamp_cost_from_app_data(Some(&valid)), Some(11));
        assert!(!pn_announce_data_is_valid(&invalid));
        assert_eq!(validate_pn_announce_data(&invalid), Err(PnAnnounceParseError::InvalidTimebase));
        assert_eq!(pn_name_from_app_data(Some(&invalid)), None);
        assert_eq!(pn_stamp_cost_from_app_data(Some(&invalid)), None);
    }
}
