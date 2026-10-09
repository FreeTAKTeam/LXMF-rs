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
    /// Changes whenever the client's response socket is replaced. Absent for
    /// legacy clients, whose reply connections must remain request-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_connection_id: Option<String>,
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
            response_connection_id: None,
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
            response_connection_id: None,
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
        if let Some(id) = &self.response_connection_id {
            if id.is_empty()
                || id.len() > 128
                || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                || self.session_id.len() > 128
                || self.response_endpoint.as_ref().is_some_and(|endpoint| endpoint.len() > 512)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid response connection routing",
                ));
            }
        }
        if self.payload.len() > codec::MAX_FRAME_PAYLOAD_LEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "zmq rpc payload too large"));
        }
        Ok(())
    }
}

pub fn encode_envelope(envelope: &ZmqRpcEnvelope) -> io::Result<Vec<u8>> {
    envelope.validate()?;
    // Named fields let older protocol-v1 peers ignore the optional routing
    // extension. Legacy seven-field tuple frames remain byte-for-byte unchanged.
    let encoded = if envelope.response_connection_id.is_some() {
        let mut encoded = vec![0; 4];
        envelope
            .serialize(&mut rmp_serde::Serializer::new(&mut encoded).with_struct_map())
            .map_err(io::Error::other)?;
        let len = u32::try_from(encoded.len() - 4).map_err(io::Error::other)?;
        encoded[..4].copy_from_slice(&len.to_be_bytes());
        encoded
    } else {
        codec::encode_frame(envelope)?
    };
    if encoded.len().saturating_sub(4) > codec::MAX_FRAME_PAYLOAD_LEN
        || encoded.len() > ZMQ_RPC_MAX_ENVELOPE_BYTES
    {
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
    fn response_connection_extension_preserves_legacy_wire_and_borrowed_routes() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct LegacyEnvelope {
            protocol_version: u16,
            session_id: String,
            request_id: u64,
            kind: ZmqRpcEnvelopeKind,
            auth: Option<ZmqRpcAuthMetadata>,
            response_endpoint: Option<String>,
            #[serde(with = "serde_bytes")]
            payload: Vec<u8>,
        }
        let legacy = LegacyEnvelope {
            protocol_version: 1,
            session_id: "compat".into(),
            request_id: 7,
            kind: ZmqRpcEnvelopeKind::Request,
            auth: None,
            response_endpoint: Some("tcp://127.0.0.1:1".into()),
            payload: codec::encode_frame(&crate::rpc::RpcRequest {
                id: 7,
                method: "status".into(),
                params: None,
            })
            .expect("frame"),
        };
        let bytes = codec::encode_frame(&legacy).expect("legacy frame");
        let mut current = decode_envelope(&bytes).expect("old client to new daemon");
        assert!(current.response_connection_id.is_none());
        assert_eq!(encode_envelope(&current).expect("encode"), bytes);
        current.response_connection_id = Some("generation-1".into());
        let extended = encode_envelope(&current).expect("extended frame");
        assert_eq!(
            codec::decode_frame::<LegacyEnvelope>(&extended).expect("new client to old daemon"),
            legacy
        );
        assert_eq!(decode_envelope(&extended).expect("new daemon"), current);
        assert_eq!(request_method(&extended).expect("admission"), "status");
        assert_eq!(request_header(&extended).expect("header"), ("compat", 7));
        assert_eq!(
            rejection_route(&extended).expect("overload route").response_connection_id,
            current.response_connection_id
        );
    }

    #[test]
    fn connection_routing_identifiers_are_bounded_and_cannot_inject_logs() {
        for id in ["".into(), "g\nsecret".into(), "a".repeat(129)] {
            let mut envelope = ZmqRpcEnvelope::request("s", 1, "tcp://127.0.0.1:1", vec![], None);
            envelope.response_connection_id = Some(id);
            assert!(encode_envelope(&envelope).is_err());
        }
    }

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
            response_connection_id: None,
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
        #[serde(default)]
        response_connection_id: Option<&'a str>,
    }
    let route: Routing<'_> = rmp_serde::from_slice(
        bytes.get(4..).ok_or_else(|| io::Error::other("missing envelope header"))?,
    )
    .map_err(io::Error::other)?;
    if route.session_id.len() > 128
        || route.response_connection_id.is_some_and(|s| {
            s.is_empty()
                || s.len() > 128
                || !s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
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
        response_connection_id: route.response_connection_id.map(str::to_owned),
    })
}
