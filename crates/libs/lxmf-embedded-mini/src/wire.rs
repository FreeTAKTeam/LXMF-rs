use heapless::Vec;

use crate::{MiniError, MiniResult};

pub const HASH_LENGTH: usize = 16;
pub const SIGNATURE_LENGTH: usize = 64;
pub const WIRE_HEADER_LENGTH: usize = HASH_LENGTH + HASH_LENGTH + SIGNATURE_LENGTH;

const MSGPACK_ARRAY_4: u8 = 0x94;
const MSGPACK_ARRAY_5: u8 = 0x95;
const MSGPACK_NIL: u8 = 0xc0;
const MSGPACK_F64: u8 = 0xcb;
const MSGPACK_BIN8: u8 = 0xc4;
const MSGPACK_BIN16: u8 = 0xc5;
const MSGPACK_BIN32: u8 = 0xc6;
const MSGPACK_STR8: u8 = 0xd9;
const MSGPACK_STR16: u8 = 0xda;
const MSGPACK_STR32: u8 = 0xdb;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MiniMessage<const TITLE: usize, const CONTENT: usize> {
    pub destination: [u8; HASH_LENGTH],
    pub source: [u8; HASH_LENGTH],
    pub signature: [u8; SIGNATURE_LENGTH],
    pub timestamp: u64,
    pub title: Vec<u8, TITLE>,
    pub content: Vec<u8, CONTENT>,
}

impl<const TITLE: usize, const CONTENT: usize> MiniMessage<TITLE, CONTENT> {
    pub fn new(
        destination: [u8; HASH_LENGTH],
        source: [u8; HASH_LENGTH],
        signature: [u8; SIGNATURE_LENGTH],
        timestamp: f64,
        title: &[u8],
        content: &[u8],
    ) -> MiniResult<Self> {
        if destination == [0; HASH_LENGTH] || source == [0; HASH_LENGTH] {
            return Err(MiniError::InvalidInput);
        }
        let mut title_buf = Vec::new();
        title_buf.extend_from_slice(title).map_err(|_| MiniError::CapacityExceeded)?;
        let mut content_buf = Vec::new();
        content_buf.extend_from_slice(content).map_err(|_| MiniError::CapacityExceeded)?;
        Ok(Self {
            destination,
            source,
            signature,
            timestamp: timestamp.to_bits(),
            title: title_buf,
            content: content_buf,
        })
    }

    pub fn timestamp(&self) -> f64 {
        f64::from_bits(self.timestamp)
    }

    pub fn encoded_len(&self) -> MiniResult<usize> {
        Ok(WIRE_HEADER_LENGTH
            + 1
            + 9
            + bin_encoded_len(self.title.len())?
            + bin_encoded_len(self.content.len())?
            + 1)
    }

    pub fn encode(&self, out: &mut [u8]) -> MiniResult<usize> {
        let needed = self.encoded_len()?;
        if out.len() < needed {
            return Err(MiniError::BufferTooSmall);
        }

        let mut idx = 0;
        write_bytes(out, &mut idx, &self.destination)?;
        write_bytes(out, &mut idx, &self.source)?;
        write_bytes(out, &mut idx, &self.signature)?;
        write_u8(out, &mut idx, MSGPACK_ARRAY_4)?;
        write_u8(out, &mut idx, MSGPACK_F64)?;
        write_bytes(out, &mut idx, &self.timestamp.to_be_bytes())?;
        write_bin(out, &mut idx, &self.title)?;
        write_bin(out, &mut idx, &self.content)?;
        write_u8(out, &mut idx, MSGPACK_NIL)?;
        Ok(idx)
    }

    pub fn decode(bytes: &[u8]) -> MiniResult<Self> {
        if bytes.len() < WIRE_HEADER_LENGTH {
            return Err(MiniError::InvalidWire);
        }

        let mut destination = [0u8; HASH_LENGTH];
        let mut source = [0u8; HASH_LENGTH];
        let mut signature = [0u8; SIGNATURE_LENGTH];
        destination.copy_from_slice(&bytes[0..HASH_LENGTH]);
        source.copy_from_slice(&bytes[HASH_LENGTH..HASH_LENGTH * 2]);
        signature.copy_from_slice(&bytes[HASH_LENGTH * 2..WIRE_HEADER_LENGTH]);

        let mut cursor = WIRE_HEADER_LENGTH;
        let array_len = read_u8(bytes, &mut cursor)?;
        if array_len == MSGPACK_ARRAY_5 {
            return Err(MiniError::Unsupported);
        }
        if array_len != MSGPACK_ARRAY_4 {
            return Err(MiniError::InvalidWire);
        }
        if read_u8(bytes, &mut cursor)? != MSGPACK_F64 {
            return Err(MiniError::InvalidWire);
        }
        let timestamp = read_u64_be(bytes, &mut cursor)?;
        let title_slice = read_bin_or_str(bytes, &mut cursor)?;
        let content_slice = read_bin_or_str(bytes, &mut cursor)?;
        if read_u8(bytes, &mut cursor)? != MSGPACK_NIL || cursor != bytes.len() {
            return Err(MiniError::InvalidWire);
        }

        let mut title = Vec::new();
        title.extend_from_slice(title_slice).map_err(|_| MiniError::CapacityExceeded)?;
        let mut content = Vec::new();
        content.extend_from_slice(content_slice).map_err(|_| MiniError::CapacityExceeded)?;
        Ok(Self { destination, source, signature, timestamp, title, content })
    }
}

fn bin_encoded_len(len: usize) -> MiniResult<usize> {
    if len <= u8::MAX as usize {
        Ok(2 + len)
    } else if len <= u16::MAX as usize {
        Ok(3 + len)
    } else if len <= u32::MAX as usize {
        Ok(5 + len)
    } else {
        Err(MiniError::InvalidInput)
    }
}

fn write_u8(out: &mut [u8], idx: &mut usize, value: u8) -> MiniResult<()> {
    if *idx >= out.len() {
        return Err(MiniError::BufferTooSmall);
    }
    out[*idx] = value;
    *idx += 1;
    Ok(())
}

fn write_bytes(out: &mut [u8], idx: &mut usize, bytes: &[u8]) -> MiniResult<()> {
    let end = idx.checked_add(bytes.len()).ok_or(MiniError::BufferTooSmall)?;
    if end > out.len() {
        return Err(MiniError::BufferTooSmall);
    }
    out[*idx..end].copy_from_slice(bytes);
    *idx = end;
    Ok(())
}

fn write_bin(out: &mut [u8], idx: &mut usize, bytes: &[u8]) -> MiniResult<()> {
    if bytes.len() <= u8::MAX as usize {
        write_u8(out, idx, MSGPACK_BIN8)?;
        write_u8(out, idx, bytes.len() as u8)?;
    } else if bytes.len() <= u16::MAX as usize {
        write_u8(out, idx, MSGPACK_BIN16)?;
        write_bytes(out, idx, &(bytes.len() as u16).to_be_bytes())?;
    } else if bytes.len() <= u32::MAX as usize {
        write_u8(out, idx, MSGPACK_BIN32)?;
        write_bytes(out, idx, &(bytes.len() as u32).to_be_bytes())?;
    } else {
        return Err(MiniError::InvalidInput);
    }
    write_bytes(out, idx, bytes)
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> MiniResult<u8> {
    let value = *bytes.get(*cursor).ok_or(MiniError::InvalidWire)?;
    *cursor += 1;
    Ok(value)
}

fn read_u64_be(bytes: &[u8], cursor: &mut usize) -> MiniResult<u64> {
    let end = cursor.checked_add(8).ok_or(MiniError::InvalidWire)?;
    let raw = bytes.get(*cursor..end).ok_or(MiniError::InvalidWire)?;
    let mut out = [0u8; 8];
    out.copy_from_slice(raw);
    *cursor = end;
    Ok(u64::from_be_bytes(out))
}

fn read_len(bytes: &[u8], cursor: &mut usize, marker: u8) -> MiniResult<usize> {
    match marker {
        MSGPACK_BIN8 | MSGPACK_STR8 => Ok(read_u8(bytes, cursor)? as usize),
        MSGPACK_BIN16 | MSGPACK_STR16 => {
            let end = cursor.checked_add(2).ok_or(MiniError::InvalidWire)?;
            let raw = bytes.get(*cursor..end).ok_or(MiniError::InvalidWire)?;
            *cursor = end;
            Ok(u16::from_be_bytes([raw[0], raw[1]]) as usize)
        }
        MSGPACK_BIN32 | MSGPACK_STR32 => {
            let end = cursor.checked_add(4).ok_or(MiniError::InvalidWire)?;
            let raw = bytes.get(*cursor..end).ok_or(MiniError::InvalidWire)?;
            *cursor = end;
            Ok(u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize)
        }
        0xa0..=0xbf => Ok((marker & 0x1f) as usize),
        _ => Err(MiniError::InvalidWire),
    }
}

fn read_bin_or_str<'a>(bytes: &'a [u8], cursor: &mut usize) -> MiniResult<&'a [u8]> {
    let marker = read_u8(bytes, cursor)?;
    let len = read_len(bytes, cursor, marker)?;
    let end = cursor.checked_add(len).ok_or(MiniError::InvalidWire)?;
    let out = bytes.get(*cursor..end).ok_or(MiniError::InvalidWire)?;
    *cursor = end;
    Ok(out)
}
