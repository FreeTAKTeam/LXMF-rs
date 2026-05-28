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
    validate_zmq_loop_config(&config, daemon.as_ref())?;
    let command_endpoint_requires_auth =
        config.require_auth_for_remote && !is_local_zmq_endpoint(&config.command_endpoint);
    let mut commands = PullSocket::new();
    commands.bind(config.command_endpoint.as_str()).await.map_err(zmq_io_error)?;
    let mut responses: HashMap<String, PushSocket> = HashMap::new();
    log::info!("reticulumd listening on zmq {}", config.command_endpoint);

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            message = commands.recv() => {
                let message = match message {
                    Ok(message) => message,
                    Err(err) if is_recoverable_zmq_transport_error(&err) => {
                        log::warn!("[daemon] zmq rpc receive dropped client connection: {}", err);
                        continue;
                    }
                    Err(err) => return Err(zmq_io_error(err)),
                };
                let response =
                    handle_zmq_command_message(
                        daemon.as_ref(),
                        message,
                        command_endpoint_requires_auth,
                    );
                if let Some(response) = response {
                    if let Err(err) = send_zmq_response(&mut responses, response).await {
                        log::warn!("[daemon] zmq rpc response dropped client connection: {}", err);
                    }
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
    command_endpoint_requires_auth: bool,
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
    let response_endpoint_is_local = is_local_zmq_endpoint(response_endpoint.as_str());
    if let Err(error) = authorize_zmq_envelope(
        daemon,
        &envelope,
        command_endpoint_requires_auth,
        response_endpoint_is_local,
    ) {
        if response_endpoint_is_local {
            return Some(ZmqOutboundResponse {
                endpoint: response_endpoint,
                envelope: rpc_error_envelope(envelope.session_id, envelope.request_id, error),
            });
        }
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

#[allow(clippy::result_large_err)]
fn authorize_zmq_envelope(
    daemon: &RpcDaemon,
    envelope: &ZmqRpcEnvelope,
    command_endpoint_requires_auth: bool,
    response_endpoint_is_local: bool,
) -> Result<(), RpcError> {
    if !command_endpoint_requires_auth
        && response_endpoint_is_local
        && !daemon.remote_rpc_auth_configured()
    {
        return Ok(());
    }
    let auth = envelope.auth.as_ref().ok_or_else(|| {
        RpcError::new("SDK_SECURITY_AUTH_REQUIRED", "zmq rpc envelope auth metadata is required")
    })?;
    if !auth.scheme.eq_ignore_ascii_case("bearer") {
        return Err(RpcError::new(
            "SDK_SECURITY_TOKEN_INVALID",
            "zmq rpc auth metadata must use bearer scheme",
        ));
    }
    let value = auth
        .value
        .strip_prefix("Bearer ")
        .or_else(|| auth.value.strip_prefix("bearer "))
        .unwrap_or(auth.value.as_str());
    let headers = vec![("authorization".to_string(), format!("Bearer {value}"))];
    daemon.authorize_http_request(&headers, Some("0.0.0.0"))
}

fn rpc_error_envelope(session_id: String, request_id: u64, error: RpcError) -> ZmqRpcEnvelope {
    let response = RpcResponse { id: request_id, result: None, error: Some(error) };
    ZmqRpcEnvelope::response(
        session_id,
        request_id,
        rns_rpc::rpc::codec::encode_frame(&response).unwrap_or_default(),
    )
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

fn validate_zmq_loop_config(config: &ZmqRpcLoopConfig, daemon: &RpcDaemon) -> io::Result<()> {
    if config.require_auth_for_remote
        && !is_local_zmq_endpoint(&config.command_endpoint)
        && !daemon.remote_rpc_token_auth_configured()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "remote zmq endpoints require explicit token authentication",
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

fn is_recoverable_zmq_transport_error(err: &zeromq::ZmqError) -> bool {
    let text = err.to_string();
    text.contains("connection was aborted")
        || text.contains("connection was forcibly closed")
        || text.contains("connection reset")
        || text.contains("(os error 10053)")
        || text.contains("(os error 10054)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rns_rpc::e2e_harness::{build_rpc_frame, parse_rpc_frame};

    #[test]
    fn config_rejects_remote_without_auth_gate() {
        let config = ZmqRpcLoopConfig {
            command_endpoint: "tcp://0.0.0.0:9100".to_string(),
            require_auth_for_remote: true,
        };
        let daemon = RpcDaemon::test_instance();

        let err = validate_zmq_loop_config(&config, &daemon).expect_err("remote bind rejected");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn config_rejects_remote_command_endpoint() {
        let config = ZmqRpcLoopConfig {
            command_endpoint: "tcp://192.0.2.10:9100".to_string(),
            require_auth_for_remote: true,
        };
        let daemon = RpcDaemon::test_instance();

        let err = validate_zmq_loop_config(&config, &daemon)
            .expect_err("remote command endpoint rejected");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn config_accepts_remote_command_endpoint_with_token_auth() {
        let config = ZmqRpcLoopConfig {
            command_endpoint: "tcp://0.0.0.0:9100".to_string(),
            require_auth_for_remote: true,
        };
        let daemon = token_auth_daemon();

        validate_zmq_loop_config(&config, &daemon).expect("token auth allows remote zmq bind");
    }

    #[test]
    fn response_endpoint_rejects_remote_endpoint() {
        let err = validate_zmq_response_endpoint("tcp://192.0.2.10:9101")
            .expect_err("remote response endpoint rejected");

        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn remote_response_endpoint_is_allowed_with_valid_token_auth() {
        let daemon = token_auth_daemon();
        let envelope = authenticated_envelope(
            "session-a",
            1,
            "tcp://192.0.2.20:9101",
            build_rpc_frame(1, "sdk_snapshot_v2", Some(serde_json::json!({}))).expect("rpc frame"),
        );

        let response = handle_zmq_command_message(
            &daemon,
            ZmqMessage::from(zmq::encode_envelope(&envelope).expect("zmq envelope")),
            true,
        )
        .expect("authenticated remote response endpoint should be accepted");

        assert_eq!(response.endpoint, "tcp://192.0.2.20:9101");
        let rpc = parse_rpc_frame(&response.envelope.payload).expect("rpc response");
        assert!(rpc.error.is_none(), "valid token auth should reach daemon RPC handler");
    }

    #[test]
    fn missing_token_auth_returns_error_to_local_response_endpoint() {
        let daemon = token_auth_daemon();
        let payload =
            build_rpc_frame(2, "sdk_snapshot_v2", Some(serde_json::json!({}))).expect("rpc frame");
        let envelope =
            ZmqRpcEnvelope::request("session-b", 2, "tcp://127.0.0.1:9101", payload, None);

        let response = handle_zmq_command_message(
            &daemon,
            ZmqMessage::from(zmq::encode_envelope(&envelope).expect("zmq envelope")),
            true,
        )
        .expect("local response endpoint should receive auth error");

        let rpc = parse_rpc_frame(&response.envelope.payload).expect("rpc response");
        let error = rpc.error.expect("auth error");
        assert_eq!(error.code, "SDK_SECURITY_AUTH_REQUIRED");
    }

    #[test]
    fn remote_command_bind_requires_auth_even_after_runtime_config_changes() {
        let daemon = RpcDaemon::test_instance();
        let payload =
            build_rpc_frame(3, "sdk_snapshot_v2", Some(serde_json::json!({}))).expect("rpc frame");
        let envelope =
            ZmqRpcEnvelope::request("session-c", 3, "tcp://127.0.0.1:9101", payload, None);

        let response = handle_zmq_command_message(
            &daemon,
            ZmqMessage::from(zmq::encode_envelope(&envelope).expect("zmq envelope")),
            true,
        )
        .expect("remote command bind should return auth error to local response endpoint");

        let rpc = parse_rpc_frame(&response.envelope.payload).expect("rpc response");
        let error = rpc.error.expect("auth error");
        assert_eq!(error.code, "SDK_SECURITY_AUTH_REQUIRED");
    }

    #[test]
    fn recoverable_zmq_transport_error_matches_client_disconnects() {
        let aborted = zeromq::ZmqError::Network(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "An established connection was aborted by the software in your host machine. (os error 10053)",
        ));
        let reset = zeromq::ZmqError::Network(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "An existing connection was forcibly closed by the remote host. (os error 10054)",
        ));
        let invalid = zeromq::ZmqError::Other("invalid endpoint");

        assert!(is_recoverable_zmq_transport_error(&aborted));
        assert!(is_recoverable_zmq_transport_error(&reset));
        assert!(!is_recoverable_zmq_transport_error(&invalid));
    }

    fn token_auth_daemon() -> RpcDaemon {
        let daemon = RpcDaemon::test_instance();
        daemon
            .configure_remote_token_auth_for_startup(
                "test-issuer",
                "test-audience",
                "test-secret",
                30_000,
                5_000,
            )
            .expect("token auth config");
        daemon
    }

    fn authenticated_envelope(
        session_id: &str,
        request_id: u64,
        response_endpoint: &str,
        payload: Vec<u8>,
    ) -> ZmqRpcEnvelope {
        let iat = unix_seconds();
        let exp = iat.saturating_add(60);
        let jti = format!("{session_id}-{request_id}");
        let signed_payload = format!(
            "iss=test-issuer;aud=test-audience;jti={jti};sub=sdk-client;iat={iat};exp={exp}"
        );
        let sig = hmac_signature("test-secret", &signed_payload);
        ZmqRpcEnvelope::request(
            session_id,
            request_id,
            response_endpoint,
            payload,
            Some(rns_rpc::rpc::zmq::ZmqRpcAuthMetadata {
                scheme: "bearer".to_string(),
                value: format!("{signed_payload};sig={sig}"),
            }),
        )
    }

    fn unix_seconds() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }

    fn hmac_signature(secret: &str, payload: &str) -> String {
        use hkdf::hmac::{Hmac, Mac};
        use sha2::Sha256;

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac secret");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}
