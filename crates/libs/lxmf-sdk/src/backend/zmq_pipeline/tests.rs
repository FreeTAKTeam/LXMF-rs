use super::*;
use rns_rpc::rpc::{RpcRequest, RpcResponse};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
struct CapturedZmqRequest {
    method: String,
    params: Option<JsonValue>,
}

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

#[test]
fn identity_announce_now_uses_zmq_sdk_method() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({ "accepted": true, "announce_id": 1 }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let ack = client.identity_announce_now().expect("identity announce");

    assert!(ack.accepted);
    assert_eq!(
        captured.lock().expect("captured request").as_ref().expect("zmq request").method,
        "sdk_identity_announce_now_v2"
    );
    server.join().expect("server joined");
}

#[test]
fn identity_presence_list_uses_zmq_sdk_method_and_decodes_response() {
    let command_endpoint = unused_loopback_endpoint();
    let response_endpoint = unused_loopback_endpoint();
    let captured = Arc::new(Mutex::new(None));
    let server = spawn_single_response_zmq_server(
        command_endpoint.clone(),
        json!({
            "presence_list": {
                "peers": [{
                    "peer_id": "peer-a",
                    "last_seen_ts_ms": 2000,
                    "first_seen_ts_ms": 1000,
                    "seen_count": 3,
                    "name": "Peer A",
                    "name_source": "announce",
                    "trust_level": "trusted",
                    "bootstrap": true,
                    "extensions": { "source": "zmq" }
                }],
                "next_cursor": "presence:1"
            }
        }),
        Arc::clone(&captured),
    );
    let mut config = ZmqPipelineBackendConfig::local_tcp(command_endpoint, response_endpoint);
    config.request_timeout = std::time::Duration::from_secs(2);
    let client = ZmqPipelineBackendClient::new(config).expect("zmq client");

    let result = client
        .identity_presence_list(crate::domain::PresenceListRequest {
            cursor: Some("presence:0".to_owned()),
            limit: Some(1),
            extensions: BTreeMap::new(),
        })
        .expect("identity presence list");

    assert_eq!(result.next_cursor.as_deref(), Some("presence:1"));
    assert_eq!(result.peers[0].peer_id, "peer-a");
    assert_eq!(result.peers[0].trust_level, Some(crate::domain::TrustLevel::Trusted));
    let captured = captured.lock().expect("captured request");
    let request = captured.as_ref().expect("zmq request");
    assert_eq!(request.method, "sdk_identity_presence_list_v2");
    assert_eq!(request.params.as_ref().expect("params")["cursor"], json!("presence:0"));
    assert_eq!(request.params.as_ref().expect("params")["limit"], json!(1));
    server.join().expect("server joined");
}

fn unused_loopback_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve tcp port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    format!("tcp://localhost:{port}")
}

fn spawn_single_response_zmq_server(
    command_endpoint: String,
    response: JsonValue,
    captured: Arc<Mutex<Option<CapturedZmqRequest>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("test runtime");
        runtime.block_on(async move {
            let mut commands = PullSocket::new();
            commands.bind(command_endpoint.as_str()).await.expect("bind command endpoint");
            let Some(envelope) = recv_request_envelope(&mut commands).await else {
                return;
            };
            let request: RpcRequest =
                rns_rpc::rpc::codec::decode_frame(&envelope.payload).expect("decode rpc request");
            *captured.lock().expect("captured request") =
                Some(CapturedZmqRequest { method: request.method, params: request.params });
            let rpc_response =
                RpcResponse { id: envelope.request_id, result: Some(response), error: None };
            let response_payload =
                rns_rpc::rpc::codec::encode_frame(&rpc_response).expect("encode rpc response");
            let response_endpoint = envelope.response_endpoint.expect("response endpoint");
            let mut responses = PushSocket::new();
            responses.connect(response_endpoint.as_str()).await.expect("connect response endpoint");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            responses
                .send(ZmqMessage::from(
                    zmq::encode_envelope(&ZmqRpcEnvelope::response(
                        envelope.session_id,
                        envelope.request_id,
                        response_payload,
                    ))
                    .expect("encode zmq response"),
                ))
                .await
                .expect("send response");
        });
    })
}

async fn recv_request_envelope(commands: &mut PullSocket) -> Option<ZmqRpcEnvelope> {
    let message = tokio::time::timeout(std::time::Duration::from_secs(1), commands.recv())
        .await
        .ok()?
        .ok()?;
    let bytes = Vec::<u8>::try_from(message).ok()?;
    zmq::decode_envelope(&bytes).ok()
}
