use std::future::Future;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{RpcRequest, RpcResponse};

pub const CONTROL_PROTOCOL_VERSION: u16 = 1;
pub const MAX_CONTROL_ENVELOPE_BYTES: usize = 4 * 1024 * 1024;
pub const CONTROL_FRAME_HEADER_BYTES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCodecError {
    Encode { message: String },
    Decode { message: String },
    UnsupportedProtocol { version: u16 },
    Oversized { len: usize, max: usize },
    IncompleteFrame { expected: usize, actual: usize },
    UnexpectedMessage { message: &'static str },
    Io { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlEnvelope {
    pub protocol_version: u16,
    pub sequence: u64,
    pub message: ControlMessage,
}

impl ControlEnvelope {
    pub fn new(sequence: u64, message: ControlMessage) -> Self {
        Self { protocol_version: CONTROL_PROTOCOL_VERSION, sequence, message }
    }

    pub fn request(sequence: u64, request: RpcRequest) -> Self {
        Self::new(sequence, ControlMessage::RpcRequest { request })
    }

    pub fn response(sequence: u64, response: RpcResponse) -> Self {
        Self::new(sequence, ControlMessage::RpcResponse { response })
    }

    pub fn encode(&self) -> Result<Vec<u8>, ControlCodecError> {
        validate_protocol_version(self.protocol_version)?;
        let encoded = rmp_serde::to_vec_named(self)
            .map_err(|err| ControlCodecError::Encode { message: err.to_string() })?;
        validate_control_envelope_size(encoded.len())?;
        Ok(encoded)
    }

    pub fn decode(data: &[u8]) -> Result<Self, ControlCodecError> {
        validate_control_envelope_size(data.len())?;
        let envelope: Self = rmp_serde::from_slice(data)
            .map_err(|err| ControlCodecError::Decode { message: err.to_string() })?;
        validate_protocol_version(envelope.protocol_version)?;
        Ok(envelope)
    }

    pub fn encode_frame(&self) -> Result<Vec<u8>, ControlCodecError> {
        encode_control_frame(&self.encode()?)
    }

    pub fn decode_frame(frame: &[u8]) -> Result<Self, ControlCodecError> {
        let payload = decode_control_frame(frame)?;
        Self::decode(payload)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    RpcRequest { request: RpcRequest },
    RpcResponse { response: RpcResponse },
    Health { role: ControlRole, ready: bool },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRole {
    Router,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlServeStopReason {
    Eof,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlServeSummary {
    pub handled_requests: usize,
    pub handled_health: usize,
    pub stop_reason: ControlServeStopReason,
}

pub async fn serve_control_router<R, W, F, Fut>(
    reader: &mut R,
    writer: &mut W,
    mut handler: F,
) -> Result<ControlServeSummary, ControlCodecError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnMut(RpcRequest) -> Fut,
    Fut: Future<Output = RpcResponse>,
{
    let mut handled_requests = 0usize;
    let mut handled_health = 0usize;
    loop {
        match read_control_envelope(reader).await {
            Ok(envelope) => match envelope.message {
                ControlMessage::RpcRequest { request } => {
                    let response = handler(request).await;
                    write_control_envelope(
                        writer,
                        &ControlEnvelope::response(envelope.sequence, response),
                    )
                    .await?;
                    handled_requests = handled_requests.saturating_add(1);
                }
                ControlMessage::Health { role: _, ready: _ } => {
                    handled_health = handled_health.saturating_add(1);
                }
                ControlMessage::Shutdown => {
                    return Ok(ControlServeSummary {
                        handled_requests,
                        handled_health,
                        stop_reason: ControlServeStopReason::Shutdown,
                    });
                }
                ControlMessage::RpcResponse { .. } => {
                    return Err(ControlCodecError::UnexpectedMessage {
                        message: "router control server received rpc response",
                    });
                }
            },
            Err(ControlCodecError::Io { message }) if is_eof_io(&message) => {
                return Ok(ControlServeSummary {
                    handled_requests,
                    handled_health,
                    stop_reason: ControlServeStopReason::Eof,
                });
            }
            Err(err) => return Err(err),
        }
    }
}

pub fn encode_control_frame(payload: &[u8]) -> Result<Vec<u8>, ControlCodecError> {
    validate_control_envelope_size(payload.len())?;
    if payload.len() > u32::MAX as usize {
        return Err(ControlCodecError::Oversized { len: payload.len(), max: u32::MAX as usize });
    }
    let mut frame = Vec::with_capacity(CONTROL_FRAME_HEADER_BYTES + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

pub fn decode_control_frame(frame: &[u8]) -> Result<&[u8], ControlCodecError> {
    if frame.len() < CONTROL_FRAME_HEADER_BYTES {
        return Err(ControlCodecError::IncompleteFrame {
            expected: CONTROL_FRAME_HEADER_BYTES,
            actual: frame.len(),
        });
    }
    let len = u32::from_be_bytes(frame[..CONTROL_FRAME_HEADER_BYTES].try_into().map_err(
        |err: std::array::TryFromSliceError| ControlCodecError::Decode { message: err.to_string() },
    )?) as usize;
    validate_control_envelope_size(len)?;
    let actual = frame.len() - CONTROL_FRAME_HEADER_BYTES;
    if actual < len {
        return Err(ControlCodecError::IncompleteFrame { expected: len, actual });
    }
    Ok(&frame[CONTROL_FRAME_HEADER_BYTES..CONTROL_FRAME_HEADER_BYTES + len])
}

pub async fn write_control_envelope<W>(
    writer: &mut W,
    envelope: &ControlEnvelope,
) -> Result<(), ControlCodecError>
where
    W: AsyncWrite + Unpin,
{
    write_control_frame(writer, &envelope.encode()?).await
}

pub async fn read_control_envelope<R>(reader: &mut R) -> Result<ControlEnvelope, ControlCodecError>
where
    R: AsyncRead + Unpin,
{
    let payload = read_control_frame(reader).await?;
    ControlEnvelope::decode(&payload)
}

pub async fn write_control_frame<W>(writer: &mut W, payload: &[u8]) -> Result<(), ControlCodecError>
where
    W: AsyncWrite + Unpin,
{
    validate_control_envelope_size(payload.len())?;
    if payload.len() > u32::MAX as usize {
        return Err(ControlCodecError::Oversized { len: payload.len(), max: u32::MAX as usize });
    }
    writer
        .write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|err| ControlCodecError::Io { message: err.to_string() })?;
    writer
        .write_all(payload)
        .await
        .map_err(|err| ControlCodecError::Io { message: err.to_string() })?;
    writer.flush().await.map_err(|err| ControlCodecError::Io { message: err.to_string() })
}

pub async fn read_control_frame<R>(reader: &mut R) -> Result<Vec<u8>, ControlCodecError>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; CONTROL_FRAME_HEADER_BYTES];
    reader
        .read_exact(&mut header)
        .await
        .map_err(|err| ControlCodecError::Io { message: err.to_string() })?;
    let len = u32::from_be_bytes(header) as usize;
    validate_control_envelope_size(len)?;
    let mut payload = vec![0u8; len];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|err| ControlCodecError::Io { message: err.to_string() })?;
    Ok(payload)
}

fn validate_protocol_version(version: u16) -> Result<(), ControlCodecError> {
    if version == CONTROL_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ControlCodecError::UnsupportedProtocol { version })
    }
}

fn validate_control_envelope_size(len: usize) -> Result<(), ControlCodecError> {
    if len <= MAX_CONTROL_ENVELOPE_BYTES {
        Ok(())
    } else {
        Err(ControlCodecError::Oversized { len, max: MAX_CONTROL_ENVELOPE_BYTES })
    }
}

fn is_eof_io(message: &str) -> bool {
    message.contains("early eof") || message.contains("failed to fill whole buffer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::RpcDaemon;
    use serde_json::json;

    fn status_request() -> RpcRequest {
        RpcRequest { id: 7, method: "daemon_status_ex".to_string(), params: Some(json!({})) }
    }

    #[test]
    fn control_envelope_round_trips_rpc_request() {
        let envelope = ControlEnvelope::request(11, status_request());
        let decoded = ControlEnvelope::decode(&envelope.encode().expect("encode")).expect("decode");
        assert_eq!(decoded, envelope);
    }

    #[test]
    fn control_envelope_rejects_wrong_protocol_version() {
        let mut envelope = ControlEnvelope::request(11, status_request());
        envelope.protocol_version = CONTROL_PROTOCOL_VERSION + 1;
        let err = envelope.encode().expect_err("unsupported protocol");
        assert!(
            matches!(err, ControlCodecError::UnsupportedProtocol { version } if version == CONTROL_PROTOCOL_VERSION + 1)
        );
    }

    #[test]
    fn control_frame_rejects_oversized_length_before_payload_allocation() {
        let oversized = (MAX_CONTROL_ENVELOPE_BYTES as u32 + 1).to_be_bytes();
        let err = decode_control_frame(&oversized).expect_err("oversized frame");
        assert!(matches!(
            err,
            ControlCodecError::Oversized { len, max }
                if len == MAX_CONTROL_ENVELOPE_BYTES + 1 && max == MAX_CONTROL_ENVELOPE_BYTES
        ));
    }

    #[test]
    fn control_frame_rejects_incomplete_payload() {
        let mut frame = Vec::new();
        frame.extend_from_slice(&8u32.to_be_bytes());
        frame.extend_from_slice(&[1, 2, 3]);
        let err = decode_control_frame(&frame).expect_err("incomplete frame");
        assert!(matches!(err, ControlCodecError::IncompleteFrame { expected: 8, actual: 3 }));
    }

    #[tokio::test]
    async fn control_envelope_moves_over_async_stream() {
        let (mut client, mut server) = tokio::io::duplex(16 * 1024);
        let sent = ControlEnvelope::request(99, status_request());
        write_control_envelope(&mut client, &sent).await.expect("write request");
        let received = read_control_envelope(&mut server).await.expect("read request");
        assert_eq!(received, sent);
    }

    #[tokio::test]
    async fn control_boundary_round_trips_request_and_response() {
        let (mut control, mut router) = tokio::io::duplex(16 * 1024);
        let request = ControlEnvelope::request(1, status_request());
        write_control_envelope(&mut control, &request).await.expect("write request");
        let received = read_control_envelope(&mut router).await.expect("read request");
        assert_eq!(received, request);

        let response = ControlEnvelope::response(
            2,
            RpcResponse { id: 7, result: Some(json!({ "ok": true })), error: None },
        );
        write_control_envelope(&mut router, &response).await.expect("write response");
        let received = read_control_envelope(&mut control).await.expect("read response");
        assert_eq!(received, response);
    }

    #[tokio::test]
    async fn control_router_serves_rpc_requests_until_shutdown() {
        let daemon = RpcDaemon::test_instance();
        let (mut control, router) = tokio::io::duplex(16 * 1024);
        let (mut router_reader, mut router_writer) = tokio::io::split(router);
        let server = tokio::spawn(async move {
            serve_control_router(&mut router_reader, &mut router_writer, |request| {
                let response = daemon.handle_rpc(request).expect("handle rpc");
                async move { response }
            })
            .await
            .expect("serve control router")
        });

        write_control_envelope(&mut control, &ControlEnvelope::request(1, status_request()))
            .await
            .expect("write request");
        let response = read_control_envelope(&mut control).await.expect("read response");
        assert_eq!(response.sequence, 1);
        let ControlMessage::RpcResponse { response } = response.message else {
            panic!("expected rpc response");
        };
        assert_eq!(response.id, 7);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        write_control_envelope(&mut control, &ControlEnvelope::new(2, ControlMessage::Shutdown))
            .await
            .expect("write shutdown");
        let summary = server.await.expect("server task");
        assert_eq!(
            summary,
            ControlServeSummary {
                handled_requests: 1,
                handled_health: 0,
                stop_reason: ControlServeStopReason::Shutdown,
            }
        );
    }

    #[tokio::test]
    async fn control_router_rejects_response_on_request_stream() {
        let (mut control, router) = tokio::io::duplex(16 * 1024);
        let (mut router_reader, mut router_writer) = tokio::io::split(router);
        let server = tokio::spawn(async move {
            serve_control_router(&mut router_reader, &mut router_writer, |request| async move {
                RpcResponse { id: request.id, result: None, error: None }
            })
            .await
            .expect_err("unexpected response should fail")
        });
        write_control_envelope(
            &mut control,
            &ControlEnvelope::response(1, RpcResponse { id: 1, result: None, error: None }),
        )
        .await
        .expect("write response");
        let err = server.await.expect("server task");
        assert!(matches!(
            err,
            ControlCodecError::UnexpectedMessage {
                message: "router control server received rpc response"
            }
        ));
    }
}
