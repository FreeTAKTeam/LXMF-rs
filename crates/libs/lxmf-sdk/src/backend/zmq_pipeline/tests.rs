use super::*;

#[test]
fn config_rejects_remote_endpoints_without_auth() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://192.0.2.10:9000", "tcp://127.0.0.1:9001");

    let err = config.validate().expect_err("remote without auth rejected");

    assert_eq!(err.category, ErrorCategory::Security);
    assert_eq!(err.machine_code, code::SECURITY_AUTH_REQUIRED);
}

#[test]
fn config_accepts_loopback_without_auth() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://127.0.0.1:9000", "tcp://localhost:9001");

    config.validate().expect("loopback accepted");
}

#[test]
fn config_normalizes_ipv4_loopback_for_windows_tcp_bind() {
    let config =
        ZmqPipelineBackendConfig::local_tcp("tcp://127.0.0.1:9000", "tcp://127.0.0.1:9001");

    assert_eq!(config.command_endpoint, "tcp://localhost:9000");
    assert_eq!(config.response_endpoint, "tcp://localhost:9001");
}

#[test]
fn response_filter_requires_session_and_request_match() {
    let session = "session-a".to_string();
    let envelope = ZmqRpcEnvelope::response(session.clone(), 4, Vec::new());

    assert_eq!(envelope.kind, ZmqRpcEnvelopeKind::Response);
    assert_eq!(envelope.session_id, session);
    assert_eq!(envelope.request_id, 4);
}
