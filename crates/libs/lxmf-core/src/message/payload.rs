use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use crate::error::LxmfError;
use alloc::format;
use alloc::string::ToString;
use alloc::vec::Vec;
use serde_bytes::Bytes;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub timestamp: f64,
    pub content: Option<ByteBuf>,
    pub title: Option<ByteBuf>,
    pub fields: Option<rmpv::Value>,
    pub stamp: Option<ByteBuf>,
}

impl Payload {
    pub fn new(
        timestamp: f64,
        content: Option<Vec<u8>>,
        title: Option<Vec<u8>>,
        fields: Option<rmpv::Value>,
        stamp: Option<Vec<u8>>,
    ) -> Self {
        Self {
            timestamp,
            content: content.map(ByteBuf::from),
            title: title.map(ByteBuf::from),
            fields,
            stamp: stamp.map(ByteBuf::from),
        }
    }

    pub fn to_msgpack(&self) -> Result<Vec<u8>, LxmfError> {
        encode_msgpack_parts(
            self.timestamp,
            self.title.as_ref().map(|value| value.as_ref()),
            self.content.as_ref().map(|value| value.as_ref()),
            self.fields.as_ref(),
            self.stamp.as_ref().map(|value| value.as_ref()),
        )
    }

    pub fn to_msgpack_without_stamp(&self) -> Result<Vec<u8>, LxmfError> {
        encode_msgpack_parts(
            self.timestamp,
            self.title.as_ref().map(|value| value.as_ref()),
            self.content.as_ref().map(|value| value.as_ref()),
            self.fields.as_ref(),
            None,
        )
    }

    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, LxmfError> {
        if let Some(payload) = decode_typed_payload(bytes)? {
            return Ok(payload);
        }

        let value = rmp_serde::from_slice::<rmpv::Value>(bytes)
            .map_err(|e| LxmfError::Decode(e.to_string()))?;
        let rmpv::Value::Array(items) = value else {
            return Err(LxmfError::Decode("invalid payload structure".into()));
        };
        if items.len() < 4 || items.len() > 5 {
            return Err(LxmfError::Decode("invalid payload length".into()));
        }
        let timestamp = items
            .first()
            .and_then(|value| value.as_f64())
            .ok_or_else(|| LxmfError::Decode("invalid payload timestamp".into()))?;
        let title = value_to_bytes(items.get(1), "title")?.map(ByteBuf::from);
        let content = value_to_bytes(items.get(2), "content")?.map(ByteBuf::from);
        let fields = match items.get(3) {
            Some(rmpv::Value::Nil) | None => None,
            Some(value) => Some(value.clone()),
        };
        let stamp = if items.len() == 5 {
            value_to_bytes(items.get(4), "stamp")?.map(ByteBuf::from)
        } else {
            None
        };
        Ok(Self { timestamp, content, title, fields, stamp })
    }
}

fn decode_typed_payload(bytes: &[u8]) -> Result<Option<Payload>, LxmfError> {
    match bytes.first().copied() {
        Some(0x94) => {
            let (timestamp, title, content, fields): (
                f64,
                Option<ByteBuf>,
                Option<ByteBuf>,
                Option<rmpv::Value>,
            ) = match rmp_serde::from_slice(bytes) {
                Ok(payload) => payload,
                Err(_) => return Ok(None),
            };
            Ok(Some(Payload { timestamp, content, title, fields, stamp: None }))
        }
        Some(0x95) => {
            let (timestamp, title, content, fields, stamp): (
                f64,
                Option<ByteBuf>,
                Option<ByteBuf>,
                Option<rmpv::Value>,
                Option<ByteBuf>,
            ) = match rmp_serde::from_slice(bytes) {
                Ok(payload) => payload,
                Err(_) => return Ok(None),
            };
            Ok(Some(Payload { timestamp, content, title, fields, stamp }))
        }
        Some(marker) if marker & 0xf0 == 0x90 => {
            Err(LxmfError::Decode("invalid payload length".into()))
        }
        _ => Ok(None),
    }
}

pub(crate) fn encode_msgpack_parts(
    timestamp: f64,
    title: Option<&[u8]>,
    content: Option<&[u8]>,
    fields: Option<&rmpv::Value>,
    stamp: Option<&[u8]>,
) -> Result<Vec<u8>, LxmfError> {
    if fields.is_none() {
        return encode_msgpack_no_fields(timestamp, title, content, stamp);
    }

    let title = title.map(Bytes::new);
    let content = content.map(Bytes::new);
    if let Some(stamp) = stamp {
        let stamp = Bytes::new(stamp);
        let list = (timestamp, title, content, fields, Some(stamp));
        rmp_serde::to_vec(&list).map_err(|e| LxmfError::Encode(e.to_string()))
    } else {
        let list = (timestamp, title, content, fields);
        rmp_serde::to_vec(&list).map_err(|e| LxmfError::Encode(e.to_string()))
    }
}

fn encode_msgpack_no_fields(
    timestamp: f64,
    title: Option<&[u8]>,
    content: Option<&[u8]>,
    stamp: Option<&[u8]>,
) -> Result<Vec<u8>, LxmfError> {
    let mut out = Vec::with_capacity(
        1 + 9
            + encoded_option_bin_len(title)
            + encoded_option_bin_len(content)
            + 1
            + stamp.map(encoded_bin_len).unwrap_or(0),
    );
    out.push(if stamp.is_some() { 0x95 } else { 0x94 });
    out.push(0xcb);
    out.extend_from_slice(&timestamp.to_bits().to_be_bytes());
    write_option_bin(&mut out, title)?;
    write_option_bin(&mut out, content)?;
    out.push(0xc0);
    if let Some(stamp) = stamp {
        write_bin(&mut out, stamp)?;
    }
    Ok(out)
}

fn encoded_option_bin_len(value: Option<&[u8]>) -> usize {
    value.map(encoded_bin_len).unwrap_or(1)
}

fn encoded_bin_len(value: &[u8]) -> usize {
    match value.len() {
        0..=0xff => 2 + value.len(),
        0x100..=0xffff => 3 + value.len(),
        _ => 5 + value.len(),
    }
}

fn write_option_bin(out: &mut Vec<u8>, value: Option<&[u8]>) -> Result<(), LxmfError> {
    if let Some(value) = value {
        write_bin(out, value)
    } else {
        out.push(0xc0);
        Ok(())
    }
}

fn write_bin(out: &mut Vec<u8>, value: &[u8]) -> Result<(), LxmfError> {
    match value.len() {
        0..=0xff => {
            out.push(0xc4);
            out.push(value.len() as u8);
        }
        0x100..=0xffff => {
            out.push(0xc5);
            out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        }
        _ => {
            let len = u32::try_from(value.len())
                .map_err(|_| LxmfError::Encode("payload field too large".into()))?;
            out.push(0xc6);
            out.extend_from_slice(&len.to_be_bytes());
        }
    }
    out.extend_from_slice(value);
    Ok(())
}

fn value_to_bytes(value: Option<&rmpv::Value>, field: &str) -> Result<Option<Vec<u8>>, LxmfError> {
    match value {
        Some(rmpv::Value::Binary(bin)) => Ok(Some(bin.clone())),
        Some(rmpv::Value::String(text)) => text
            .as_str()
            .map(|s| Some(s.as_bytes().to_vec()))
            .ok_or_else(|| LxmfError::Decode(format!("invalid payload {field} string"))),
        Some(rmpv::Value::Nil) | None => Ok(None),
        _ => Err(LxmfError::Decode(format!("invalid payload {field}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_fields_fast_path_matches_serde_without_stamp() {
        let timestamp = 1_774_001_500.25;
        let title = b"fast-title";
        let content = b"fast-content";
        let encoded = encode_msgpack_parts(timestamp, Some(title), Some(content), None, None)
            .expect("fast path encode");
        let expected = rmp_serde::to_vec(&(
            timestamp,
            Some(Bytes::new(title)),
            Some(Bytes::new(content)),
            Option::<rmpv::Value>::None,
        ))
        .expect("serde encode");

        assert_eq!(encoded, expected);
    }

    #[test]
    fn no_fields_fast_path_matches_serde_with_stamp_and_nil_values() {
        let timestamp = 1_774_001_600.5;
        let stamp = [0x5a; 32];
        let encoded = encode_msgpack_parts(timestamp, None, None, None, Some(&stamp))
            .expect("fast path encode");
        let expected = rmp_serde::to_vec(&(
            timestamp,
            Option::<&Bytes>::None,
            Option::<&Bytes>::None,
            Option::<rmpv::Value>::None,
            Some(Bytes::new(&stamp)),
        ))
        .expect("serde encode");

        assert_eq!(encoded, expected);
    }

    #[test]
    fn no_fields_fast_path_uses_bin16_for_large_content() {
        let timestamp = 1_774_001_700.75;
        let content = vec![0x42; 2048];
        let encoded = encode_msgpack_parts(timestamp, Some(b"title"), Some(&content), None, None)
            .expect("fast path encode");
        let decoded = Payload::from_msgpack(&encoded).expect("decode fast path payload");

        assert_eq!(decoded.timestamp, timestamp);
        assert_eq!(decoded.title.as_ref().map(|value| value.as_ref()), Some(&b"title"[..]));
        assert_eq!(decoded.content.as_ref().map(|value| value.as_ref()), Some(content.as_slice()));
        assert_eq!(decoded.fields, None);
        assert_eq!(decoded.stamp, None);
    }
}
