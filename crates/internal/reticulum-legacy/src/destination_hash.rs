use std::io::{Error, ErrorKind};

pub fn parse_destination_hash(input: &str) -> Option<[u8; 16]> {
    let bytes = hex::decode(input.trim()).ok()?;
    let mut out = [0u8; 16];
    match bytes.len() {
        16 => {
            out.copy_from_slice(&bytes);
            Some(out)
        }
        32 => {
            out.copy_from_slice(&bytes[..16]);
            Some(out)
        }
        _ => None,
    }
}

pub fn parse_destination_hash_required(input: &str) -> Result<[u8; 16], Error> {
    parse_destination_hash(input).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("invalid destination hash '{input}' (expected 16-byte or 32-byte hex)"),
        )
    })
}
