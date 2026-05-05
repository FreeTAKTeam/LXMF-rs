use crate::{EmbeddedError, EmbeddedResult};
use alloc::vec::Vec;

const MAGIC: &[u8; 4] = b"RNE1";
const VERSION: u8 = 0x01;
const HEADER_LEN: usize = 14;
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PacketFrame {
    pub kind: u8,
    pub sequence: u32,
    pub payload: Vec<u8>,
}

impl PacketFrame {
    pub fn new(kind: u8, sequence: u32, payload: Vec<u8>) -> EmbeddedResult<Self> {
        if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(Self { kind, sequence, payload })
    }
}

pub fn encode_frame(frame: &PacketFrame) -> EmbeddedResult<Vec<u8>> {
    if frame.payload.is_empty() || frame.payload.len() > MAX_PAYLOAD_BYTES {
        return Err(EmbeddedError::InvalidInput);
    }
    let payload_len_u32 =
        u32::try_from(frame.payload.len()).map_err(|_| EmbeddedError::InvalidInput)?;

    let mut out = Vec::with_capacity(HEADER_LEN + frame.payload.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(frame.kind);
    out.extend_from_slice(&frame.sequence.to_le_bytes());
    out.extend_from_slice(&payload_len_u32.to_le_bytes());
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

pub fn decode_frame(bytes: &[u8]) -> EmbeddedResult<PacketFrame> {
    if bytes.len() < HEADER_LEN {
        return Err(EmbeddedError::InvalidInput);
    }
    if &bytes[0..4] != MAGIC {
        return Err(EmbeddedError::IntegrityFailure);
    }
    if bytes[4] != VERSION {
        return Err(EmbeddedError::Unsupported);
    }
    let kind = bytes[5];
    let sequence = u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]);
    let payload_len = u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]);
    let payload_len = usize::try_from(payload_len).map_err(|_| EmbeddedError::InvalidInput)?;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
        return Err(EmbeddedError::InvalidInput);
    }
    if bytes.len() != HEADER_LEN + payload_len {
        return Err(EmbeddedError::InvalidInput);
    }
    let payload = bytes[HEADER_LEN..].to_vec();
    PacketFrame::new(kind, sequence, payload)
}

#[cfg(test)]
mod tests {
    use super::{decode_frame, encode_frame, PacketFrame};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct PacketFixture {
        id: String,
        kind: u8,
        sequence: u32,
        payload_hex: String,
        encoded_hex: String,
    }

    #[test]
    fn fixture_vectors_roundtrip() {
        let fixtures: Vec<PacketFixture> = serde_json::from_str(include_str!(
            "../../../../docs/fixtures/embedded/native_packet_vectors.json"
        ))
        .expect("fixture json parse");
        for fixture in fixtures {
            let payload = hex::decode(&fixture.payload_hex).expect("payload hex");
            let expected = hex::decode(&fixture.encoded_hex).expect("encoded hex");

            let frame = PacketFrame::new(fixture.kind, fixture.sequence, payload.clone())
                .expect("frame init");
            let encoded = encode_frame(&frame).expect("encode");
            assert_eq!(encoded, expected, "fixture {} encode mismatch", fixture.id);

            let decoded = decode_frame(&expected).expect("decode");
            assert_eq!(decoded.kind, fixture.kind, "fixture {} kind mismatch", fixture.id);
            assert_eq!(
                decoded.sequence, fixture.sequence,
                "fixture {} sequence mismatch",
                fixture.id
            );
            assert_eq!(decoded.payload, payload, "fixture {} payload mismatch", fixture.id);
        }
    }

    #[test]
    fn rejects_truncated_frame() {
        let bytes = vec![0_u8; 8];
        assert!(decode_frame(&bytes).is_err());
    }
}
