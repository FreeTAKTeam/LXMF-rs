use std::io::{self, ErrorKind};

use rmp_serde::{from_slice, to_vec};
use serde::{de::DeserializeOwned, Serialize};

pub fn encode_frame<T: Serialize>(msg: &T) -> io::Result<Vec<u8>> {
    let payload = to_vec(msg).map_err(|err| io::Error::new(ErrorKind::InvalidData, err))?;
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "frame too large"))?;
    let mut framed = Vec::with_capacity(4 + payload.len());
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

pub fn decode_frame<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    if bytes.len() < 4 {
        return Err(io::Error::new(ErrorKind::UnexpectedEof, "missing frame header"));
    }
    let mut len_buf = [0u8; 4];
    len_buf.copy_from_slice(&bytes[..4]);
    let len = u32::from_be_bytes(len_buf) as usize;
    if bytes.len() < 4 + len {
        return Err(io::Error::new(ErrorKind::UnexpectedEof, "incomplete frame"));
    }
    let payload = &bytes[4..4 + len];
    from_slice(payload).map_err(|err| io::Error::new(ErrorKind::InvalidData, err))
}
