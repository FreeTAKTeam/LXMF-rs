//! Strict RFC 8949 §4.2.1 deterministic CBOR for the RPP / RDO subset:
//! unsigned integers, byte strings, UTF-8 text, arrays, uint-keyed maps and
//! (only where the caller allows it) null. Anything else is refused.

use std::collections::BTreeMap;

use crate::error::{Result, RppError};

pub const MAX_DEPTH: usize = 12;
pub const MAX_TEXT_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Uint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(BTreeMap<u64, Value>),
    Null,
}

impl Value {
    pub fn as_uint(&self) -> Result<u64> {
        match self {
            Self::Uint(value) => Ok(*value),
            _ => Err(RppError::Malformed("expected unsigned integer")),
        }
    }

    pub fn into_bytes(self) -> Result<Vec<u8>> {
        match self {
            Self::Bytes(value) => Ok(value),
            _ => Err(RppError::Malformed("expected byte string")),
        }
    }

    pub fn into_fixed<const N: usize>(self) -> Result<[u8; N]> {
        self.into_bytes()?.try_into().map_err(|_| RppError::Malformed("wrong byte-string length"))
    }

    pub fn into_text(self) -> Result<String> {
        match self {
            Self::Text(value) => Ok(value),
            _ => Err(RppError::Malformed("expected text")),
        }
    }

    pub fn into_array(self) -> Result<Vec<Value>> {
        match self {
            Self::Array(value) => Ok(value),
            _ => Err(RppError::Malformed("expected array")),
        }
    }

    pub fn into_map(self) -> Result<BTreeMap<u64, Value>> {
        match self {
            Self::Map(value) => Ok(value),
            _ => Err(RppError::Malformed("expected map")),
        }
    }
}

pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    out
}

fn encode_head(major: u8, value: u64, out: &mut Vec<u8>) {
    let major = major << 5;
    // Shortest form; the `as` casts are guarded by the range checks.
    if value < 24 {
        out.push(major | value as u8);
    } else if value <= u64::from(u8::MAX) {
        out.extend_from_slice(&[major | 24, value as u8]);
    } else if value <= u64::from(u16::MAX) {
        out.push(major | 25);
        out.extend_from_slice(&(value as u16).to_be_bytes());
    } else if value <= u64::from(u32::MAX) {
        out.push(major | 26);
        out.extend_from_slice(&(value as u32).to_be_bytes());
    } else {
        out.push(major | 27);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn encode_into(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Uint(value) => encode_head(0, *value, out),
        Value::Bytes(bytes) => {
            encode_head(2, bytes.len() as u64, out);
            out.extend_from_slice(bytes);
        }
        Value::Text(text) => {
            encode_head(3, text.len() as u64, out);
            out.extend_from_slice(text.as_bytes());
        }
        Value::Array(items) => {
            encode_head(4, items.len() as u64, out);
            items.iter().for_each(|item| encode_into(item, out));
        }
        Value::Map(entries) => {
            // Shortest-form uint keys sort bytewise exactly as they sort numerically.
            encode_head(5, entries.len() as u64, out);
            for (key, item) in entries {
                encode_head(0, *key, out);
                encode_into(item, out);
            }
        }
        Value::Null => out.push(0xf6),
    }
}

/// Decodes exactly one deterministic item occupying all of `bytes`.
pub fn decode(bytes: &[u8], max_len: usize, allow_null: bool) -> Result<Value> {
    if bytes.len() > max_len {
        return Err(RppError::LimitExceeded);
    }
    let mut decoder = Decoder { bytes, pos: 0, allow_null };
    let value = decoder.item(0)?;
    if decoder.pos != bytes.len() {
        return Err(RppError::Malformed("trailing bytes"));
    }
    Ok(value)
}

struct Decoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    allow_null: bool,
}

impl Decoder<'_> {
    fn take(&mut self, len: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(len).filter(|end| *end <= self.bytes.len());
        let end = end.ok_or(RppError::Malformed("truncated"))?;
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    fn head(&mut self) -> Result<(u8, u64)> {
        let initial = self.take(1)?[0];
        let (major, info) = (initial >> 5, initial & 0x1f);
        if major == 7 {
            return Ok((7, u64::from(info)));
        }
        let (value, minimum) = match info {
            0..=23 => return Ok((major, u64::from(info))),
            24 => (u64::from(self.take(1)?[0]), 24),
            25 => (u64::from(u16::from_be_bytes(fixed(self.take(2)?))), 1 << 8),
            26 => (u64::from(u32::from_be_bytes(fixed(self.take(4)?))), 1 << 16),
            27 => (u64::from_be_bytes(fixed(self.take(8)?)), 1 << 32),
            31 => return Err(RppError::Malformed("indefinite length")),
            _ => return Err(RppError::Malformed("reserved additional information")),
        };
        if value < minimum {
            return Err(RppError::Noncanonical("overlong integer or length"));
        }
        Ok((major, value))
    }

    fn length(&self, declared: u64) -> Result<usize> {
        // Every item needs at least one byte; refuse before allocating.
        usize::try_from(declared)
            .ok()
            .filter(|len| *len <= self.remaining())
            .ok_or(RppError::Malformed("declared length exceeds input"))
    }

    fn item(&mut self, depth: usize) -> Result<Value> {
        let (major, argument) = self.head()?;
        match major {
            0 => Ok(Value::Uint(argument)),
            1 => Err(RppError::Malformed("negative integer")),
            2 => {
                let len = self.length(argument)?;
                Ok(Value::Bytes(self.take(len)?.to_vec()))
            }
            3 => {
                let len = self.length(argument)?;
                if len > MAX_TEXT_BYTES {
                    return Err(RppError::LimitExceeded);
                }
                let text = std::str::from_utf8(self.take(len)?)
                    .map_err(|_| RppError::Malformed("invalid UTF-8"))?;
                Ok(Value::Text(text.to_owned()))
            }
            4 | 5 => {
                if depth >= MAX_DEPTH {
                    return Err(RppError::Malformed("nesting too deep"));
                }
                let count = self.length(argument)?;
                if major == 4 {
                    let mut items = Vec::with_capacity(count.min(256));
                    for _ in 0..count {
                        items.push(self.item(depth + 1)?);
                    }
                    return Ok(Value::Array(items));
                }
                let mut entries = BTreeMap::new();
                let mut previous: Option<u64> = None;
                for _ in 0..count {
                    let (key_major, key) = self.head()?;
                    if key_major != 0 {
                        return Err(RppError::Malformed("map key is not an unsigned integer"));
                    }
                    match previous {
                        Some(previous) if key == previous => {
                            return Err(RppError::Malformed("duplicate map key"))
                        }
                        Some(previous) if key < previous => {
                            return Err(RppError::Noncanonical("map keys out of order"))
                        }
                        _ => {}
                    }
                    previous = Some(key);
                    entries.insert(key, self.item(depth + 1)?);
                }
                Ok(Value::Map(entries))
            }
            6 => Err(RppError::Malformed("tags are not allowed")),
            _ if argument == 22 && self.allow_null => Ok(Value::Null),
            _ => Err(RppError::Malformed("float, simple value or null not allowed")),
        }
    }
}

fn fixed<const N: usize>(slice: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(slice);
    out
}

/// Consumes the fields of a uint-keyed map; `finish` refuses leftovers.
pub struct Fields(BTreeMap<u64, Value>);

impl Fields {
    pub fn new(map: BTreeMap<u64, Value>) -> Self {
        Self(map)
    }

    pub fn take(&mut self, key: u64) -> Option<Value> {
        self.0.remove(&key)
    }

    pub fn required(&mut self, key: u64) -> Result<Value> {
        self.take(key).ok_or(RppError::Malformed("missing required field"))
    }

    pub fn uint(&mut self, key: u64) -> Result<u64> {
        self.required(key)?.as_uint()
    }

    pub fn opt_uint(&mut self, key: u64) -> Result<Option<u64>> {
        self.take(key).map(|value| value.as_uint()).transpose()
    }

    pub fn fixed<const N: usize>(&mut self, key: u64) -> Result<[u8; N]> {
        self.required(key)?.into_fixed()
    }

    pub fn opt_fixed<const N: usize>(&mut self, key: u64) -> Result<Option<[u8; N]>> {
        self.take(key).map(Value::into_fixed).transpose()
    }

    pub fn opt_bytes(&mut self, key: u64, max: usize) -> Result<Option<Vec<u8>>> {
        let Some(bytes) = self.take(key).map(Value::into_bytes).transpose()? else {
            return Ok(None);
        };
        if bytes.len() > max {
            return Err(RppError::LimitExceeded);
        }
        Ok(Some(bytes))
    }

    pub fn opt_text(&mut self, key: u64) -> Result<Option<String>> {
        self.take(key).map(Value::into_text).transpose()
    }

    pub fn array(&mut self, key: u64) -> Result<Vec<Value>> {
        self.required(key)?.into_array()
    }

    pub fn finish(self) -> Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(RppError::Malformed("unassigned field"))
        }
    }
}

/// Builds a uint-keyed map from `(key, value)` pairs, skipping `None`.
pub fn map(entries: impl IntoIterator<Item = (u64, Option<Value>)>) -> Value {
    Value::Map(entries.into_iter().filter_map(|(key, value)| Some((key, value?))).collect())
}

pub fn uint_array<I: IntoIterator<Item = u64>>(values: I) -> Value {
    Value::Array(values.into_iter().map(Value::Uint).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_rejects_noncanonical_forms() {
        let value = map([
            (0, Some(Value::Uint(500))),
            (24, Some(Value::Bytes(vec![1, 2]))),
            (31, Some(Value::Array(vec![Value::Text("a".into())]))),
        ]);
        let bytes = encode(&value);
        assert_eq!(decode(&bytes, 64, false), Ok(value));
        assert!(matches!(decode(&[0x18, 0x01], 8, false), Err(RppError::Noncanonical(_))));
        assert!(matches!(
            decode(&[0xa2, 0x01, 0x00, 0x00, 0x00], 8, false),
            Err(RppError::Noncanonical(_))
        ));
        assert!(matches!(decode(&[0x20], 8, false), Err(RppError::Malformed(_))));
        assert!(matches!(decode(&[0xf6], 8, false), Err(RppError::Malformed(_))));
        assert_eq!(decode(&[0xf6], 8, true), Ok(Value::Null));
        assert!(matches!(
            decode(&[0x5a, 0xff, 0xff, 0xff, 0xff], 8, false),
            Err(RppError::Malformed(_))
        ));
        let deep = [[0x81u8; 13].as_slice(), &[0x00]].concat();
        assert!(matches!(decode(&deep, 64, false), Err(RppError::Malformed(_))));
    }

    /// Nested arrays that each claim ~4M items must not reserve memory for
    /// them up front (~1.5 GB before the fix); decoding fails on the filler.
    #[test]
    fn claimed_array_counts_do_not_preallocate() {
        let mut input = [0x9a, 0x00, 0x3f, 0xff, 0xf0].repeat(MAX_DEPTH);
        input.resize(4 * 1024 * 1024, 0xff);
        assert!(matches!(decode(&input, 4 * 1024 * 1024, false), Err(RppError::Malformed(_))));
    }
}
