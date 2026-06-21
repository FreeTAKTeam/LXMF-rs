use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

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
            Self::InvalidDisplayName => f.write_str("display name is empty or contains control characters"),
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

impl fmt::Display for AnnounceParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
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

pub fn encode_delivery_display_name_app_data(display_name: &str) -> Result<Vec<u8>, AnnounceEncodeError> {
    let normalized = normalize_display_name(display_name).ok_or(AnnounceEncodeError::InvalidDisplayName)?;
    let peer_data =
        rmpv::Value::Array(vec![rmpv::Value::Binary(normalized.into_bytes()), rmpv::Value::Nil]);
    Ok(encode_msgpack(&peer_data)?)
}

pub fn display_name_from_delivery_app_data(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }

    let decoded: rmpv::Value = decode_msgpack(data)?;
    match decoded {
        rmpv::Value::Array(values) => {
            let first = values.first()?;
            match first {
                rmpv::Value::Binary(bytes) => {
                    let raw = decode_utf8_owned(bytes.clone())?;
                    normalize_display_name(raw.as_str())
                }
                rmpv::Value::String(value) => normalize_display_name(value.as_str()?),
                _ => None,
            }
        }
        rmpv::Value::Binary(bytes) => {
            let raw = decode_utf8_owned(bytes)?;
            normalize_display_name(raw.as_str())
        }
        rmpv::Value::String(value) => normalize_display_name(value.as_str()?),
        _ => None,
    }
}

fn encode_msgpack(value: &rmpv::Value) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    rmp_serde::to_vec(value)
}

fn decode_msgpack<T>(data: &[u8]) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let decoded = rmp_serde::from_slice(data);
    decoded.ok()
}

fn decode_utf8_owned(data: Vec<u8>) -> Option<String> {
    let text = String::from_utf8(data);
    text.ok()
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
