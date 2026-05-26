#![allow(dead_code)]

use rns_rpc::rpc::zmq::{self, ZmqRpcEnvelope, ZmqRpcEnvelopeKind};
use rns_rpc::{RpcDaemon, RpcError, RpcResponse};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::sync::watch;
use zeromq::{PullSocket, PushSocket, Socket, SocketRecv, SocketSend, ZmqMessage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ZmqRpcLoopConfig {
    pub command_endpoint: String,
    pub require_auth_for_remote: bool,
}

pub(super) async fn run_zmq_rpc_loop_until(
    config: ZmqRpcLoopConfig,
    daemon: Arc<RpcDaemon>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    validate_zmq_loop_config(&config)?;
    let mut commands = PullSocket::new();
    commands.bind(config.command_endpoint.as_str()).await.map_err(zmq_io_error)?;
    let mut responses: HashMap<String, PushSocket> = HashMap::new();
    println!("reticulumd listening on zmq {}", config.command_endpoint);

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            message = commands.recv() => {
                let response =
                    handle_zmq_command_message(daemon.as_ref(), message.map_err(zmq_io_error)?);
                if let Some(response) = response {
                    send_zmq_response(&mut responses, response).await?;
                }
            }
        }
    }
    Ok(())
}

struct ZmqOutboundResponse {
    endpoint: String,
    envelope: ZmqRpcEnvelope,
}

async fn send_zmq_response(
    responses: &mut HashMap<String, PushSocket>,
    response: ZmqOutboundResponse,
) -> io::Result<()> {
    if !responses.contains_key(&response.endpoint) {
        let mut socket = PushSocket::new();
        socket.connect(response.endpoint.as_str()).await.map_err(zmq_io_error)?;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        responses.insert(response.endpoint.clone(), socket);
    }
    let socket = responses
        .get_mut(&response.endpoint)
        .ok_or_else(|| io::Error::other("missing zmq response socket"))?;
    let encoded = zmq::encode_envelope(&response.envelope)?;
    socket.send(ZmqMessage::from(encoded)).await.map_err(zmq_io_error)
}

fn handle_zmq_command_message(
    daemon: &RpcDaemon,
    message: ZmqMessage,
) -> Option<ZmqOutboundResponse> {
    let bytes = match Vec::<u8>::try_from(message) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };
    let envelope = match zmq::decode_envelope(&bytes) {
        Ok(envelope) => envelope,
        Err(_) => return None,
    };
    let response_endpoint = envelope.response_endpoint.clone()?;
    if validate_zmq_response_endpoint(response_endpoint.as_str()).is_err() {
        return None;
    }
    if envelope.kind != ZmqRpcEnvelopeKind::Request {
        return Some(ZmqOutboundResponse {
            endpoint: response_endpoint,
            envelope: error_envelope(
                envelope.session_id,
                envelope.request_id,
                "SDK_TRANSPORT_ZMQ_INVALID_KIND",
                "zmq command ingress accepts request envelopes only",
            ),
        });
    }
    let response_payload =
        daemon.handle_framed_request(envelope.payload.as_slice()).unwrap_or_else(|err| {
            let response = RpcResponse {
                id: envelope.request_id,
                result: None,
                error: Some(RpcError::new("SDK_INTERNAL", err.to_string())),
            };
            rns_rpc::rpc::codec::encode_frame(&response).unwrap_or_default()
        });
    Some(ZmqOutboundResponse {
        endpoint: response_endpoint,
        envelope: ZmqRpcEnvelope::response(
            envelope.session_id,
            envelope.request_id,
            response_payload,
        ),
    })
}

fn error_envelope(
    session_id: impl Into<String>,
    request_id: u64,
    code: &'static str,
    message: impl Into<String>,
) -> ZmqRpcEnvelope {
    let response = RpcResponse {
        id: request_id,
        result: None,
        error: Some(RpcError::new(code, message.into())),
    };
    ZmqRpcEnvelope::response(
        session_id.into(),
        request_id,
        rns_rpc::rpc::codec::encode_frame(&response).unwrap_or_default(),
    )
}

fn validate_zmq_loop_config(config: &ZmqRpcLoopConfig) -> io::Result<()> {
    if config.require_auth_for_remote && !is_local_zmq_endpoint(&config.command_endpoint) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "remote zmq endpoints require explicit authentication",
        ));
    }
    Ok(())
}

fn validate_zmq_response_endpoint(endpoint: &str) -> io::Result<()> {
    if is_local_zmq_endpoint(endpoint) {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "remote zmq response endpoints require explicit authentication",
    ))
}

fn is_local_zmq_endpoint(endpoint: &str) -> bool {
    endpoint.starts_with("inproc://")
        || endpoint.starts_with("tcp://127.")
        || endpoint.starts_with("tcp://localhost:")
        || endpoint.starts_with("tcp://[::1]:")
}

fn zmq_io_error(err: impl std::fmt::Display) -> io::Error {
    io::Error::other(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_remote_without_auth_gate() {
        let config = ZmqRpcLoopConfig {
            command_endpoint: "tcp://0.0.0.0:9100".to_string(),
            require_auth_for_remote: true,
        };

        let err = validate_zmq_loop_config(&config).expect_err("remote bind rejected");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn config_rejects_remote_command_endpoint() {
        let config = ZmqRpcLoopConfig {
            command_endpoint: "tcp://192.0.2.10:9100".to_string(),
            require_auth_for_remote: true,
        };

        let err = validate_zmq_loop_config(&config).expect_err("remote command endpoint rejected");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn response_endpoint_rejects_remote_endpoint() {
        let err = validate_zmq_response_endpoint("tcp://192.0.2.10:9101")
            .expect_err("remote response endpoint rejected");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
