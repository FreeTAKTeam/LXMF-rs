use super::codec;
use serde::{Deserialize, Serialize};
use std::io;

pub const ZMQ_RPC_PROTOCOL_VERSION: u16 = 1;
pub const ZMQ_RPC_MAX_ENVELOPE_BYTES: usize = codec::MAX_FRAME_PAYLOAD_LEN + 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZmqRpcEnvelopeKind {
    Request,
    Response,
    Event,
    Control,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZmqRpcAuthMetadata {
    pub scheme: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZmqRpcEnvelope {
    pub protocol_version: u16,
    pub session_id: String,
    pub request_id: u64,
    pub kind: ZmqRpcEnvelopeKind,
    #[serde(default)]
    pub auth: Option<ZmqRpcAuthMetadata>,
    #[serde(default)]
    pub response_endpoint: Option<String>,
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

impl ZmqRpcEnvelope {
    pub fn request(
        session_id: impl Into<String>,
        request_id: u64,
        response_endpoint: impl Into<String>,
        payload: Vec<u8>,
        auth: Option<ZmqRpcAuthMetadata>,
    ) -> Self {
        Self {
            protocol_version: ZMQ_RPC_PROTOCOL_VERSION,
            session_id: session_id.into(),
            request_id,
            kind: ZmqRpcEnvelopeKind::Request,
            auth,
            response_endpoint: Some(response_endpoint.into()),
            payload,
        }
    }

    pub fn response(session_id: String, request_id: u64, payload: Vec<u8>) -> Self {
        Self {
            protocol_version: ZMQ_RPC_PROTOCOL_VERSION,
            session_id,
            request_id,
            kind: ZmqRpcEnvelopeKind::Response,
            auth: None,
            response_endpoint: None,
            payload,
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.protocol_version != ZMQ_RPC_PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported zmq rpc protocol version {}", self.protocol_version),
            ));
        }
        if self.session_id.trim().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "missing zmq rpc session_id"));
        }
        if self.payload.len() > codec::MAX_FRAME_PAYLOAD_LEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "zmq rpc payload too large"));
        }
        Ok(())
    }
}

pub fn encode_envelope(envelope: &ZmqRpcEnvelope) -> io::Result<Vec<u8>> {
    envelope.validate()?;
    let encoded = codec::encode_frame(envelope)?;
    if encoded.len() > ZMQ_RPC_MAX_ENVELOPE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "zmq rpc envelope too large"));
    }
    Ok(encoded)
}

pub fn decode_envelope(bytes: &[u8]) -> io::Result<ZmqRpcEnvelope> {
    if bytes.len() > ZMQ_RPC_MAX_ENVELOPE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "zmq rpc envelope too large"));
    }
    let envelope: ZmqRpcEnvelope = codec::decode_frame(bytes)?;
    envelope.validate()?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zmq_rpc_envelope_roundtrips_framed_rpc_payload() {
        let payload = codec::encode_frame(&crate::rpc::RpcRequest {
            id: 7,
            method: "sdk_snapshot_v2".to_string(),
            params: None,
        })
        .expect("rpc frame");
        let envelope =
            ZmqRpcEnvelope::request("session-a", 7, "tcp://127.0.0.1:9124", payload, None);

        let encoded = encode_envelope(&envelope).expect("encode envelope");
        let decoded = decode_envelope(&encoded).expect("decode envelope");

        assert_eq!(decoded, envelope);
    }

    #[test]
    fn zmq_rpc_envelope_rejects_wrong_protocol_version() {
        let envelope = ZmqRpcEnvelope {
            protocol_version: ZMQ_RPC_PROTOCOL_VERSION + 1,
            session_id: "session-a".to_string(),
            request_id: 1,
            kind: ZmqRpcEnvelopeKind::Request,
            auth: None,
            response_endpoint: Some("tcp://127.0.0.1:9124".to_string()),
            payload: Vec::new(),
        };

        let err = encode_envelope(&envelope).expect_err("bad version rejected");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
