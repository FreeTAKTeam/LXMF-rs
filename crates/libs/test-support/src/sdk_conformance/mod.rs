use lxmf_sdk::{
    CancelResult, Client, ConfigPatch, EventCursor, LxmfSdk, LxmfSdkAsync, MessageId,
    RpcBackendClient, SendRequest, StartRequest, SubscriptionStart,
};
use rns_rpc::e2e_harness::{
    build_http_post, build_rpc_frame, parse_http_response_body, parse_rpc_frame,
};
use rns_rpc::storage::messages::MessagesStore;
use rns_rpc::{http, RpcDaemon, RpcEvent, RpcResponse};
use serde_json::{json, Value as JsonValue};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

mod auth_mode_tests;
mod release_bc_tests;

const EVENT_LOG_OVERFLOW_TRIGGER: usize = 1_100;
const RPC_IO_TIMEOUT_SECS: u64 = 10;
static RPC_HARNESS_SERIAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn rpc_harness_serial_lock() -> &'static Mutex<()> {
    RPC_HARNESS_SERIAL_LOCK.get_or_init(|| Mutex::new(()))
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n").map(|idx| idx + 4)
}

fn parse_content_length(header_bytes: &[u8]) -> Option<usize> {
    let headers = std::str::from_utf8(header_bytes).ok()?;
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            value.trim().parse::<usize>().ok()
        } else {
            None
        }
    })
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    let mut target_len: Option<usize> = None;

    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                request.extend_from_slice(&chunk[..n]);
                if target_len.is_none() {
                    if let Some(header_end) = find_header_end(&request) {
                        let content_len = parse_content_length(&request[..header_end]).unwrap_or(0);
                        target_len = Some(header_end + content_len);
                    }
                }
                if let Some(target_len) = target_len {
                    if request.len() >= target_len {
                        break;
                    }
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(err) => return Err(err),
        }
    }

    Ok(request)
}

struct RpcHarness {
    _serial_guard: MutexGuard<'static, ()>,
    endpoint: String,
    daemon: Arc<Mutex<RpcDaemon>>,
    stop: Arc<AtomicBool>,
    next_request_id: AtomicU64,
    join: Option<JoinHandle<()>>,
}

impl RpcHarness {
    fn new() -> Self {
        let serial_guard = rpc_harness_serial_lock().lock().unwrap_or_else(|err| err.into_inner());
        let daemon = Arc::new(Mutex::new(RpcDaemon::with_store(
            MessagesStore::in_memory().expect("in-memory message store"),
            "sdk-test-runtime".to_owned(),
        )));

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind rpc harness listener");
        listener.set_nonblocking(true).expect("set listener non-blocking");
        let endpoint = listener.local_addr().expect("listener addr").to_string();

        let stop = Arc::new(AtomicBool::new(false));
        let daemon_for_thread = Arc::clone(&daemon);
        let stop_for_thread = Arc::clone(&stop);

        let join = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, addr)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ =
                            stream.set_read_timeout(Some(Duration::from_secs(RPC_IO_TIMEOUT_SECS)));
                        let _ = stream
                            .set_write_timeout(Some(Duration::from_secs(RPC_IO_TIMEOUT_SECS)));
                        let request = match read_http_request(&mut stream) {
                            Ok(request) => request,
                            Err(_) => continue,
                        };
                        if request.is_empty() {
                            continue;
                        }
                        let response = {
                            let guard =
                                daemon_for_thread.lock().unwrap_or_else(|err| err.into_inner());
                            http::handle_http_request_with_peer(&guard, &request, Some(addr))
                        }
                        .unwrap_or_else(|_| {
                            b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".to_vec()
                        });
                        let _ = stream.write_all(&response);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            _serial_guard: serial_guard,
            endpoint,
            daemon,
            stop,
            next_request_id: AtomicU64::new(1),
            join: Some(join),
        }
    }

    fn client(&self) -> Client<RpcBackendClient> {
        Client::new(RpcBackendClient::new(self.endpoint.clone()))
    }

    fn emit_event(&self, event_type: &str, payload: JsonValue) {
        self.daemon
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .emit_event(RpcEvent { event_type: event_type.to_owned(), payload });
    }

    fn rpc_call(&self, method: &str, params: Option<JsonValue>) -> RpcResponse {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let frame = build_rpc_frame(request_id, method, params).expect("encode rpc frame");
        let request = build_http_post("/rpc", &self.endpoint, &frame);

        let mut stream = TcpStream::connect(&self.endpoint).expect("connect harness endpoint");
        stream
            .set_read_timeout(Some(Duration::from_secs(RPC_IO_TIMEOUT_SECS)))
            .expect("set rpc read timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(RPC_IO_TIMEOUT_SECS)))
            .expect("set rpc write timeout");
        stream.write_all(&request).expect("write rpc request");
        stream.shutdown(std::net::Shutdown::Write).expect("shutdown write side");

        let mut raw_response = Vec::new();
        stream.read_to_end(&mut raw_response).expect("read rpc response");
        let body = parse_http_response_body(&raw_response).expect("parse response body");
        parse_rpc_frame(&body).expect("decode rpc response frame")
    }
}

impl Drop for RpcHarness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(&self.endpoint);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn base_start_request() -> StartRequest {
    serde_json::from_value(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": [],
        "config": {
            "profile": "desktop-full",
            "bind_mode": "local_only",
            "auth_mode": "local_trusted",
            "overflow_policy": "reject",
            "event_stream": {
                "max_poll_events": 256,
                "max_event_bytes": 65536,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32
            },
            "idempotency_ttl_ms": 86400000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false
            }
        }
    }))
    .expect("deserialize start request")
}

fn insecure_remote_start_request() -> StartRequest {
    serde_json::from_value(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": [],
        "config": {
            "profile": "desktop-full",
            "bind_mode": "remote",
            "auth_mode": "local_trusted",
            "overflow_policy": "reject",
            "event_stream": {
                "max_poll_events": 256,
                "max_event_bytes": 65536,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32
            },
            "idempotency_ttl_ms": 86400000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false
            }
        }
    }))
    .expect("deserialize insecure remote start request")
}

fn token_without_config_start_request() -> StartRequest {
    serde_json::from_value(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": [],
        "config": {
            "profile": "desktop-full",
            "bind_mode": "remote",
            "auth_mode": "token",
            "overflow_policy": "reject",
            "event_stream": {
                "max_poll_events": 256,
                "max_event_bytes": 65536,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32
            },
            "idempotency_ttl_ms": 86400000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false
            }
        }
    }))
    .expect("deserialize token-mode start request")
}

fn token_remote_start_request() -> StartRequest {
    serde_json::from_value(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": [],
        "config": {
            "profile": "desktop-full",
            "bind_mode": "remote",
            "auth_mode": "token",
            "overflow_policy": "reject",
            "event_stream": {
                "max_poll_events": 256,
                "max_event_bytes": 65536,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32
            },
            "idempotency_ttl_ms": 86400000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false
            },
            "rpc_backend": {
                "listen_addr": "127.0.0.1:0",
                "read_timeout_ms": 2000,
                "write_timeout_ms": 2000,
                "max_header_bytes": 8192,
                "max_body_bytes": 1048576,
                "token_auth": {
                    "issuer": "sdk-test",
                    "audience": "rns-rpc",
                    "jti_cache_ttl_ms": 60000,
                    "clock_skew_ms": 0,
                    "shared_secret": "sdk-shared-secret"
                }
            }
        }
    }))
    .expect("deserialize token remote start request")
}

fn mtls_remote_start_request() -> StartRequest {
    serde_json::from_value(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": [],
        "config": {
            "profile": "desktop-full",
            "bind_mode": "remote",
            "auth_mode": "mtls",
            "overflow_policy": "reject",
            "event_stream": {
                "max_poll_events": 256,
                "max_event_bytes": 65536,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32
            },
            "idempotency_ttl_ms": 86400000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false
            },
            "rpc_backend": {
                "listen_addr": "127.0.0.1:0",
                "read_timeout_ms": 2000,
                "write_timeout_ms": 2000,
                "max_header_bytes": 8192,
                "max_body_bytes": 1048576,
                "mtls_auth": {
                    "ca_bundle_path": "/tmp/sdk-ca.pem",
                    "require_client_cert": true,
                    "allowed_san": "urn:test-san"
                }
            }
        }
    }))
    .expect("deserialize mtls remote start request")
}

fn send_request(payload_content: &str, idempotency_key: Option<&str>) -> SendRequest {
    serde_json::from_value(json!({
        "source": "source.test",
        "destination": "destination.test",
        "payload": {
            "title": "test payload",
            "content": payload_content
        },
        "idempotency_key": idempotency_key,
        "ttl_ms": null,
        "correlation_id": null,
        "extensions": {}
    }))
    .expect("deserialize send request")
}

fn overflow_patch() -> ConfigPatch {
    serde_json::from_value(json!({
        "overflow_policy": "reject"
    }))
    .expect("deserialize overflow patch")
}

#[test]
fn sdk_conformance_negotiation_success_and_no_overlap_failure() {
    let harness = RpcHarness::new();
    let client = harness.client();
    let handle = client.start(base_start_request()).expect("start with compatible capabilities");
    assert_eq!(handle.active_contract_version, 2);
    assert!(!handle.runtime_id.is_empty());

    let incompatible_client = harness.client();
    let mut incompatible_request = base_start_request();
    incompatible_request.requested_capabilities = vec!["sdk.capability.not_supported".to_owned()];
    let err = incompatible_client
        .start(incompatible_request)
        .expect_err("start must fail when no capability overlap exists");
    assert_eq!(err.machine_code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");
}

#[test]
fn sdk_conformance_idempotent_send_reuses_message_id() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let first = client.send(send_request("payload-a", Some("idem-key"))).expect("first send");
    let second = client.send(send_request("payload-a", Some("idem-key"))).expect("deduped send");
    assert_eq!(first, second);
}

#[test]
fn sdk_conformance_idempotency_conflict_is_rejected() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    client.send(send_request("payload-a", Some("idem-key"))).expect("first send");
    let err = client
        .send(send_request("payload-b", Some("idem-key")))
        .expect_err("same idempotency key with different payload must fail");
    assert_eq!(err.machine_code, "SDK_VALIDATION_IDEMPOTENCY_CONFLICT");
}

#[test]
fn sdk_conformance_poll_cursor_monotonicity_and_invalid_cursor() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    harness.emit_event("health_snapshot", json!({ "status": "ok", "idx": 1 }));
    harness.emit_event("health_snapshot", json!({ "status": "ok", "idx": 2 }));

    let first = client.poll_events(None, 1).expect("first poll");
    assert_eq!(first.events.len(), 1);
    let first_seq = first.events[0].seq_no;
    let second =
        client.poll_events(Some(first.next_cursor.clone()), 1).expect("second poll with cursor");
    assert_eq!(second.events.len(), 1);
    assert!(second.events[0].seq_no > first_seq);

    let err = client
        .poll_events(Some(EventCursor("invalid-cursor-token".to_owned())), 1)
        .expect_err("invalid cursor must fail");
    assert_eq!(err.machine_code, "SDK_RUNTIME_INVALID_CURSOR");
}

#[test]
fn sdk_conformance_stream_gap_is_emitted_after_log_overflow() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    for idx in 0..EVENT_LOG_OVERFLOW_TRIGGER {
        harness.emit_event("flood", json!({ "idx": idx }));
    }

    let batch = client.poll_events(None, 8).expect("poll with overflow");
    assert!(!batch.events.is_empty(), "batch should include stream gap event");
    assert!(
        batch.events.iter().any(|event| event.event_type == "StreamGap"),
        "batch should contain StreamGap"
    );
    assert!(batch.dropped_count > 0, "dropped_count should report overflow");
}

#[test]
fn sdk_conformance_subscribe_events_tail_starts_from_current_end() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    harness.emit_event("seed_event", json!({ "idx": 1 }));
    harness.emit_event("seed_event", json!({ "idx": 2 }));

    let subscription =
        client.subscribe_events(SubscriptionStart::Tail).expect("subscribe with tail start");
    let first =
        client.poll_events(subscription.cursor.clone(), 16).expect("poll using tail cursor");
    assert!(
        first.events.iter().all(|event| event.event_type != "seed_event"),
        "tail subscription should skip backlog events"
    );

    harness.emit_event("live_event", json!({ "idx": 3 }));
    let second = client.poll_events(Some(first.next_cursor.clone()), 16).expect("poll live events");
    assert!(
        second.events.iter().any(|event| event.event_type == "live_event"),
        "tail cursor should include events emitted after subscription"
    );
}

#[test]
fn sdk_conformance_cancel_accepted_and_too_late_paths() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let pending_message_id = "pending-cancel-message";
    let receive_response = harness.rpc_call(
        "receive_message",
        Some(json!({
            "id": pending_message_id,
            "source": "source.test",
            "destination": "destination.test",
            "title": "",
            "content": "inbound message for cancel test",
            "fields": null
        })),
    );
    assert!(receive_response.error.is_none(), "receive_message should succeed");

    let cancel_result = client.cancel(MessageId(pending_message_id.to_owned())).expect("cancel");
    assert_eq!(cancel_result, CancelResult::Accepted);

    let sent_id = client.send(send_request("already-sent", None)).expect("send");
    let sent_id_raw = sent_id.0.clone();
    let receipt_response = harness.rpc_call(
        "record_receipt",
        Some(json!({
            "message_id": sent_id_raw,
            "status": "sent",
        })),
    );
    assert!(receipt_response.error.is_none(), "record_receipt should succeed");
    let too_late = client.cancel(sent_id).expect("cancel too late path");
    assert_eq!(too_late, CancelResult::TooLateToCancel);
}

#[test]
fn sdk_conformance_configure_cas_conflict() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let first = client.configure(0, overflow_patch()).expect("first configure");
    assert!(first.accepted);
    assert_eq!(first.revision, Some(1));

    let err = client.configure(0, overflow_patch()).expect_err("stale revision must fail");
    assert_eq!(err.machine_code, "SDK_CONFIG_CONFLICT");
}

#[test]
fn sdk_conformance_snapshot_tracks_event_position() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    harness.emit_event("policy_changed", json!({ "scope": "delivery" }));

    let snapshot = client.snapshot().expect("snapshot");
    assert_eq!(snapshot.active_contract_version, 2);
    assert!(snapshot.event_stream_position > 0);
}

#[test]
fn sdk_conformance_poll_rejects_max_over_limit() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let err = client.poll_events(None, 257).expect_err("poll max above negotiated limit must fail");
    assert_eq!(err.machine_code, "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED");
}

#[test]
fn sdk_conformance_sent_terminality_depends_on_receipt_capability() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let message_id = client.send(send_request("terminality", None)).expect("send");
    let message_id_raw = message_id.0.clone();
    let response = harness.rpc_call(
        "record_receipt",
        Some(json!({
            "message_id": message_id_raw,
            "status": "sent",
        })),
    );
    assert!(response.error.is_none(), "record_receipt should succeed");
    let snapshot = client
        .status(MessageId(message_id.0.clone()))
        .expect("status")
        .expect("message should exist");
    assert!(!snapshot.terminal, "sent must be non-terminal with receipt_terminality");
}
