use crate::{EmbeddedError, EmbeddedResult};
use alloc::{string::String, vec::Vec};

const MAGIC: &[u8; 4] = b"ELX1";
const VERSION: u8 = 0x01;
const HEADER_LEN: usize = 49;
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct MinimalEnvelope {
    pub source: [u8; 16],
    pub destination: [u8; 16],
    pub sequence: u64,
    pub body: Vec<u8>,
}

impl MinimalEnvelope {
    pub fn validate(&self) -> EmbeddedResult<()> {
        if self.source == [0; 16] || self.destination == [0; 16] {
            return Err(EmbeddedError::InvalidInput);
        }
        if self.body.is_empty() || self.body.len() > MAX_BODY_BYTES {
            return Err(EmbeddedError::InvalidInput);
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        alloc::format!(
            "src={:02x?} dst={:02x?} seq={} bytes={}",
            &self.source[..4],
            &self.destination[..4],
            self.sequence,
            self.body.len()
        )
    }
}

pub fn encode_envelope(envelope: &MinimalEnvelope) -> EmbeddedResult<Vec<u8>> {
    envelope.validate()?;
    let body_len_u32 = u32::try_from(envelope.body.len()).map_err(|_| EmbeddedError::InvalidInput)?;

    let mut out = Vec::with_capacity(HEADER_LEN + envelope.body.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&envelope.source);
    out.extend_from_slice(&envelope.destination);
    out.extend_from_slice(&envelope.sequence.to_le_bytes());
    out.extend_from_slice(&body_len_u32.to_le_bytes());
    out.extend_from_slice(&envelope.body);
    Ok(out)
}

pub fn decode_envelope(bytes: &[u8]) -> EmbeddedResult<MinimalEnvelope> {
    if bytes.len() < HEADER_LEN {
        return Err(EmbeddedError::InvalidInput);
    }
    if &bytes[0..4] != MAGIC {
        return Err(EmbeddedError::IntegrityFailure);
    }
    if bytes[4] != VERSION {
        return Err(EmbeddedError::Unsupported);
    }

    let mut source = [0_u8; 16];
    source.copy_from_slice(&bytes[5..21]);
    let mut destination = [0_u8; 16];
    destination.copy_from_slice(&bytes[21..37]);
    let sequence = u64::from_le_bytes([
        bytes[37], bytes[38], bytes[39], bytes[40], bytes[41], bytes[42], bytes[43], bytes[44],
    ]);
    let body_len = u32::from_le_bytes([bytes[45], bytes[46], bytes[47], bytes[48]]);
    let body_len = usize::try_from(body_len).map_err(|_| EmbeddedError::InvalidInput)?;
    if body_len == 0 || body_len > MAX_BODY_BYTES {
        return Err(EmbeddedError::InvalidInput);
    }
    if bytes.len() != HEADER_LEN + body_len {
        return Err(EmbeddedError::InvalidInput);
    }
    let body = bytes[HEADER_LEN..].to_vec();

    let envelope = MinimalEnvelope {
        source,
        destination,
        sequence,
        body,
    };
    envelope.validate()?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::{MinimalEnvelope, decode_envelope, encode_envelope};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct LxmfFixture {
        id: String,
        source_hex: String,
        destination_hex: String,
        sequence: u64,
        body_hex: String,
        encoded_hex: String,
    }

    #[test]
    fn fixture_vectors_roundtrip() {
        let fixtures: Vec<LxmfFixture> = serde_json::from_str(include_str!(
            "../../../../docs/fixtures/embedded/native_lxmf_min_vectors.json"
        ))
        .expect("fixture parse");
        for fixture in fixtures {
            let source = hex::decode(&fixture.source_hex).expect("source hex");
            let destination = hex::decode(&fixture.destination_hex).expect("destination hex");
            let body = hex::decode(&fixture.body_hex).expect("body hex");
            let expected = hex::decode(&fixture.encoded_hex).expect("encoded hex");

            let mut source_arr = [0_u8; 16];
            source_arr.copy_from_slice(&source);
            let mut destination_arr = [0_u8; 16];
            destination_arr.copy_from_slice(&destination);

            let envelope = MinimalEnvelope {
                source: source_arr,
                destination: destination_arr,
                sequence: fixture.sequence,
                body: body.clone(),
            };

            let encoded = encode_envelope(&envelope).expect("encode");
            assert_eq!(encoded, expected, "fixture {} encode mismatch", fixture.id);

            let decoded = decode_envelope(&expected).expect("decode");
            assert_eq!(decoded, envelope, "fixture {} decode mismatch", fixture.id);
        }
    }
}
