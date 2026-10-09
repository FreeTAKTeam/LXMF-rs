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

/// Borrow only the method for admission classification, without allocating
/// authentication strings, payload bytes or JSON parameters before admission.
pub fn request_admission(bytes: &[u8]) -> io::Result<(&str, usize)> {
    #[derive(Deserialize)]
    struct Envelope<'a> {
        protocol_version: u16,
        session_id: &'a str,
        #[serde(rename = "request_id")]
        _request_id: u64,
        kind: ZmqRpcEnvelopeKind,
        #[serde(default, rename = "auth")]
        _auth: serde::de::IgnoredAny,
        #[serde(default, rename = "response_endpoint")]
        _response_endpoint: serde::de::IgnoredAny,
        #[serde(borrow, with = "serde_bytes")]
        payload: &'a [u8],
    }
    #[derive(Deserialize)]
    struct Method<'a> {
        #[serde(rename = "id")]
        _id: u64,
        method: &'a str,
        #[serde(default, rename = "params")]
        _params: serde::de::IgnoredAny,
    }
    fn payload(bytes: &[u8]) -> io::Result<&[u8]> {
        let header: [u8; 4] = bytes
            .get(..4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "missing frame header"))?
            .try_into()
            .map_err(io::Error::other)?;
        let len = u32::from_be_bytes(header) as usize;
        if len > codec::MAX_FRAME_PAYLOAD_LEN || bytes.len() != len + 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid frame size"));
        }
        Ok(&bytes[4..])
    }
    if bytes.len() > ZMQ_RPC_MAX_ENVELOPE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "zmq envelope too large"));
    }
    let envelope: Envelope<'_> = rmp_serde::from_slice(payload(bytes)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if envelope.protocol_version != ZMQ_RPC_PROTOCOL_VERSION
        || envelope.kind != ZmqRpcEnvelopeKind::Request
        || envelope.session_id.trim().is_empty()
    {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid request envelope"));
    }
    let nodes = super::zmq_complexity::check(payload(envelope.payload)?)?;
    let method: Method<'_> = rmp_serde::from_slice(payload(envelope.payload)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let retained = bytes.len().saturating_mul(8).saturating_add(nodes.saturating_mul(256));
    Ok((method.method, retained))
}

pub fn request_method(bytes: &[u8]) -> io::Result<&str> {
    request_admission(bytes).map(|(method, _)| method)
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

/// Small borrowed reply-routing metadata for bounded overload replies.
pub fn request_header(bytes: &[u8]) -> io::Result<(&str, u64)> {
    #[derive(Deserialize)]
    struct Header<'a> {
        #[serde(rename = "protocol_version")]
        _version: u16,
        session_id: &'a str,
        request_id: u64,
        #[serde(rename = "kind")]
        _kind: serde::de::IgnoredAny,
        #[serde(default, rename = "auth")]
        _auth: serde::de::IgnoredAny,
        #[serde(default, rename = "response_endpoint")]
        _response: serde::de::IgnoredAny,
        #[serde(rename = "payload")]
        _payload: serde::de::IgnoredAny,
    }
    if bytes.len() < 4 || bytes.len() > ZMQ_RPC_MAX_ENVELOPE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid envelope size"));
    }
    let header: Header<'_> = rmp_serde::from_slice(&bytes[4..]).map_err(io::Error::other)?;
    if header.session_id.len() > 128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "overload reply session identifier too large",
        ));
    }
    Ok((header.session_id, header.request_id))
}

/// Borrow ingress metadata and ignore the message body before admitting a small rejection.
pub fn rejection_route(bytes: &[u8]) -> io::Result<ZmqRpcEnvelope> {
    #[derive(Deserialize)]
    struct Auth<'a> {
        scheme: &'a str,
        value: &'a str,
    }
    #[derive(Deserialize)]
    struct Routing<'a> {
        protocol_version: u16,
        session_id: &'a str,
        request_id: u64,
        kind: ZmqRpcEnvelopeKind,
        #[serde(default, borrow)]
        auth: Option<Auth<'a>>,
        #[serde(default)]
        response_endpoint: Option<&'a str>,
        #[serde(rename = "payload")]
        _payload: serde::de::IgnoredAny,
    }
    let route: Routing<'_> = rmp_serde::from_slice(
        bytes.get(4..).ok_or_else(|| io::Error::other("missing envelope header"))?,
    )
    .map_err(io::Error::other)?;
    if route.session_id.len() > 128
        || route.response_endpoint.is_some_and(|s| s.len() > 512)
        || route.auth.as_ref().is_some_and(|a| a.value.len() > 4096 || a.scheme.len() > 32)
    {
        return Err(io::Error::other("reply routing metadata exceeded rejection bound"));
    }
    Ok(ZmqRpcEnvelope {
        protocol_version: route.protocol_version,
        session_id: route.session_id.to_owned(),
        request_id: route.request_id,
        kind: route.kind,
        auth: route
            .auth
            .map(|a| ZmqRpcAuthMetadata { scheme: a.scheme.to_owned(), value: a.value.to_owned() }),
        response_endpoint: route.response_endpoint.map(str::to_owned),
        payload: Vec::new(),
    })
}
