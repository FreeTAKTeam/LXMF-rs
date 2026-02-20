use std::io::{self, ErrorKind};

use rmp_serde::{from_slice, Serializer};
use serde::{de::DeserializeOwned, Serialize};

pub fn encode_frame<T: Serialize>(msg: &T) -> io::Result<Vec<u8>> {
    // Reserve 4 bytes for the length prefix and serialize directly into the output frame
    // to avoid building a temporary payload buffer.
    let mut framed = Vec::with_capacity(512);
    framed.extend_from_slice(&[0u8; 4]);
    msg.serialize(&mut Serializer::new(&mut framed))
        .map_err(|err| io::Error::new(ErrorKind::InvalidData, err))?;
    let payload_len = framed
        .len()
        .checked_sub(4)
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "missing frame payload"))?;
    let len = u32::try_from(payload_len)
        .map_err(|_| io::Error::new(ErrorKind::InvalidData, "frame too large"))?;
    framed[..4].copy_from_slice(&len.to_be_bytes());
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

#[cfg(test)]
mod tests {
    use super::{decode_frame, encode_frame};
    use serde::{Deserialize, Serialize};
    use std::io;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Probe {
        id: u32,
        label: String,
    }

    #[test]
    fn encode_frame_prefixes_payload_length_and_roundtrips() {
        let probe = Probe { id: 7, label: "ready".to_string() };
        let encoded = encode_frame(&probe).expect("encode frame");
        assert!(encoded.len() > 4);

        let mut header = [0u8; 4];
        header.copy_from_slice(&encoded[..4]);
        let len = u32::from_be_bytes(header) as usize;
        assert_eq!(len + 4, encoded.len());

        let decoded: Probe = decode_frame(&encoded).expect("decode frame");
        assert_eq!(decoded, probe);
    }

    #[test]
    fn decode_frame_rejects_short_or_incomplete_frames() {
        let err = decode_frame::<Probe>(&[1, 2, 3]).expect_err("short header should fail");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);

        let mut incomplete = vec![0, 0, 0, 8];
        incomplete.extend_from_slice(&[1, 2, 3, 4]);
        let err = decode_frame::<Probe>(&incomplete).expect_err("incomplete payload should fail");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}
