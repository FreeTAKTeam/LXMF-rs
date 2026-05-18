use lxmf_sdk::{
    required_capabilities, Ack, CancelResult, Client, ConfigPatch, DeliverySnapshot, DeliveryState,
    EventBatch, EventCursor, EventSubscription, GroupSendRequest, LxmfSdk, LxmfSdkAsync,
    LxmfSdkGroupDelivery, MessageId, NegotiationRequest, NegotiationResponse, OverflowPolicy,
    Profile, RpcBackendClient, RuntimeSnapshot, RuntimeState, SdkBackend, SdkBackendAsyncEvents,
    SdkError, SdkEvent, SdkEventStream, SendRequest, Severity, ShutdownMode, StartRequest,
    SubscriptionStart, TickBudget, TickResult,
};
use rns_rpc::e2e_harness::{
    build_http_post, build_rpc_frame, parse_http_response_body, parse_rpc_frame, timestamp_millis,
};
use rns_rpc::rpc::codec;
use rns_rpc::{http, MessagesStore, RpcDaemon, RpcEvent, RpcRequest, RpcResponse};
use serde_json::{json, Value as JsonValue};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

mod app_mode_contract_tests;
mod auth_mode_tests;
mod certification_tests;
mod crypto_agility_tests;
mod key_management_tests;
mod model_tests;
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

fn query_cursor(path: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == "cursor" && !value.is_empty()).then(|| value.to_string())
    })
}

fn sdk_from_json<T: serde::de::DeserializeOwned>(value: JsonValue) -> T {
    serde_json::from_value(value).expect("conformance fixture json must decode")
}

fn sdk_event(seq_no: u64, event_type: &str) -> SdkEvent {
    sdk_from_json(json!({
        "event_id": format!("conformance-event-{seq_no}"),
        "runtime_id": "conformance-runtime",
        "stream_id": "sdk-events-v2",
        "seq_no": seq_no,
        "contract_version": 2,
        "ts_ms": seq_no,
        "event_type": event_type,
        "severity": Severity::Info,
        "source_component": "sdk-conformance",
        "operation_id": null,
        "message_id": null,
        "peer_id": null,
        "correlation_id": null,
        "trace_id": null,
        "payload": { "idx": seq_no },
        "extensions": {}
    }))
}

fn conformance_negotiation() -> NegotiationResponse {
    let mut effective_capabilities = required_capabilities(Profile::DesktopFull)
        .iter()
        .map(|capability| (*capability).to_owned())
        .collect::<Vec<_>>();
    effective_capabilities.push("sdk.capability.cursor_replay".to_owned());
    effective_capabilities.push("sdk.capability.async_events".to_owned());
    effective_capabilities.sort();
    effective_capabilities.dedup();
    sdk_from_json(json!({
        "runtime_id": "conformance-runtime",
        "active_contract_version": 2,
        "effective_capabilities": effective_capabilities,
        "effective_limits": {
            "max_poll_events": 256,
            "max_event_bytes": 65_536,
            "max_batch_bytes": 1_048_576,
            "max_extension_keys": 32,
            "idempotency_ttl_ms": 86_400_000
        },
        "contract_release": "v2.5",
        "schema_namespace": "v2"
    }))
}

struct AppStreamState {
    live_events: Mutex<VecDeque<SdkEvent>>,
    catchup_events: Mutex<VecDeque<SdkEvent>>,
    poll_cursors: Mutex<Vec<Option<EventCursor>>>,
}

#[derive(Clone)]
struct AppStreamConformanceBackend {
    state: Arc<AppStreamState>,
}

impl AppStreamConformanceBackend {
    fn new(live_events: Vec<SdkEvent>, catchup_events: Vec<SdkEvent>) -> Self {
        Self {
            state: Arc::new(AppStreamState {
                live_events: Mutex::new(VecDeque::from(live_events)),
                catchup_events: Mutex::new(VecDeque::from(catchup_events)),
                poll_cursors: Mutex::new(Vec::new()),
            }),
        }
    }
}

struct SlowConsumerStats {
    attempted_sends: AtomicUsize,
    completed_sends: AtomicUsize,
}

#[derive(Clone)]
struct SlowConsumerConformanceBackend {
    event_count: u64,
    stats: Arc<SlowConsumerStats>,
}

impl SlowConsumerConformanceBackend {
    fn new(event_count: u64, stats: Arc<SlowConsumerStats>) -> Self {
        Self { event_count, stats }
    }
}

fn event_batch(events: Vec<SdkEvent>, cursor_seq: u64) -> EventBatch {
    sdk_from_json(json!({
        "events": events,
        "next_cursor": format!("v2:conformance-runtime:sdk-events-v2:{cursor_seq}"),
        "dropped_count": 0,
        "snapshot_high_watermark_seq_no": null,
        "extensions": {}
    }))
}

macro_rules! impl_conformance_backend_base {
    ($backend:ty) => {
        impl SdkBackend for $backend {
            fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
                Ok(conformance_negotiation())
            }

            fn send(&self, _req: SendRequest) -> Result<MessageId, SdkError> {
                Ok(MessageId("conformance-message".to_owned()))
            }

            fn cancel(&self, _id: MessageId) -> Result<CancelResult, SdkError> {
                Ok(CancelResult::Accepted)
            }

            fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
                Ok(Some(sdk_from_json(json!({
                    "message_id": id,
                    "state": DeliveryState::Sent,
                    "terminal": false,
                    "last_updated_ms": 0,
                    "attempts": 1,
                    "reason_code": null
                }))))
            }

            fn configure(
                &self,
                _expected_revision: u64,
                _patch: ConfigPatch,
            ) -> Result<Ack, SdkError> {
                Ok(sdk_from_json(json!({ "accepted": true, "revision": 1 })))
            }

            fn poll_events(
                &self,
                _cursor: Option<EventCursor>,
                _max: usize,
            ) -> Result<EventBatch, SdkError> {
                Ok(event_batch(Vec::new(), 0))
            }

            fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
                Ok(sdk_from_json(json!({
                    "runtime_id": "conformance-runtime",
                    "state": RuntimeState::Running,
                    "active_contract_version": 2,
                    "event_stream_position": 0,
                    "config_revision": 0,
                    "queued_messages": 0,
                    "in_flight_messages": 0
                })))
            }

            fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
                Ok(sdk_from_json(json!({ "accepted": true, "revision": null })))
            }

            fn tick(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
                Ok(sdk_from_json(json!({
                    "processed_items": 0,
                    "yielded": false,
                    "next_recommended_delay_ms": null
                })))
            }
        }
    };
}

impl_conformance_backend_base!(SlowConsumerConformanceBackend);

impl SdkBackend for AppStreamConformanceBackend {
    fn negotiate(&self, _req: NegotiationRequest) -> Result<NegotiationResponse, SdkError> {
        Ok(conformance_negotiation())
    }

    fn send(&self, _req: SendRequest) -> Result<MessageId, SdkError> {
        Ok(MessageId("conformance-message".to_owned()))
    }

    fn cancel(&self, _id: MessageId) -> Result<CancelResult, SdkError> {
        Ok(CancelResult::Accepted)
    }

    fn status(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        Ok(Some(sdk_from_json(json!({
            "message_id": id,
            "state": DeliveryState::Sent,
            "terminal": false,
            "last_updated_ms": 0,
            "attempts": 1,
            "reason_code": null
        }))))
    }

    fn configure(&self, _expected_revision: u64, _patch: ConfigPatch) -> Result<Ack, SdkError> {
        Ok(sdk_from_json(json!({ "accepted": true, "revision": 1 })))
    }

    fn poll_events(&self, cursor: Option<EventCursor>, max: usize) -> Result<EventBatch, SdkError> {
        self.state.poll_cursors.lock().expect("poll cursors mutex").push(cursor);
        let mut catchup_events = self.state.catchup_events.lock().expect("catchup events mutex");
        let events = (0..max).filter_map(|_| catchup_events.pop_front()).collect::<Vec<_>>();
        let next_seq = events.last().map_or(0, |event| event.seq_no);
        Ok(event_batch(events, next_seq))
    }

    fn snapshot(&self) -> Result<RuntimeSnapshot, SdkError> {
        Ok(sdk_from_json(json!({
            "runtime_id": "conformance-runtime",
            "state": RuntimeState::Running,
            "active_contract_version": 2,
            "event_stream_position": 0,
            "config_revision": 0,
            "queued_messages": 0,
            "in_flight_messages": 0
        })))
    }

    fn shutdown(&self, _mode: ShutdownMode) -> Result<Ack, SdkError> {
        Ok(sdk_from_json(json!({ "accepted": true, "revision": null })))
    }

    fn tick(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
        Ok(sdk_from_json(json!({
            "processed_items": 0,
            "yielded": false,
            "next_recommended_delay_ms": null
        })))
    }
}

impl SdkBackendAsyncEvents for AppStreamConformanceBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        Ok(sdk_from_json(json!({ "start": start, "cursor": null })))
    }

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let events =
            self.state.live_events.lock().expect("live events mutex").drain(..).collect::<Vec<_>>();
        Ok(Some(Box::pin(tokio_stream::iter(events.into_iter().map(Ok)))))
    }
}

impl SdkBackendAsyncEvents for SlowConsumerConformanceBackend {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        Ok(sdk_from_json(json!({ "start": start, "cursor": null })))
    }

    fn open_event_stream(
        &self,
        _subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let stats = Arc::clone(&self.stats);
        let event_count = self.event_count;
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<SdkEvent, SdkError>>(1);
        tokio::spawn(async move {
            for seq in 1..=event_count {
                stats.attempted_sends.fetch_add(1, Ordering::Relaxed);
                if tx.send(Ok(sdk_event(seq, "slow_consumer_probe"))).await.is_err() {
                    break;
                }
                stats.completed_sends.fetch_add(1, Ordering::Relaxed);
            }
        });
        Ok(Some(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))))
    }
}

fn handle_event_stream_once(daemon: &RpcDaemon, request: &[u8]) -> std::io::Result<Vec<u8>> {
    let (_method, path, _headers) = http::request_method_path_headers(request)?;
    let cursor = query_cursor(path.as_str());
    let response = daemon.handle_rpc(RpcRequest {
        id: 0,
        method: "sdk_poll_events_v2".to_string(),
        params: Some(json!({ "cursor": cursor, "max": 256 })),
    })?;
    if response.error.is_some() {
        return Ok(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".to_vec());
    }

    let mut http_response =
        b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n".to_vec();
    if let Some(events) = response
        .result
        .as_ref()
        .and_then(|result| result.get("events"))
        .and_then(JsonValue::as_array)
    {
        for event in events {
            http_response.extend(codec::encode_frame(event)?);
        }
    }
    Ok(http_response)
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
                            match http::request_method_path_headers(&request) {
                                Ok((method, path, _headers))
                                    if method == "GET"
                                        && path.split('?').next() == Some("/events/stream") =>
                                {
                                    handle_event_stream_once(&guard, &request)
                                }
                                _ => http::handle_http_request_with_peer(
                                    &guard,
                                    &request,
                                    Some(addr),
                                ),
                            }
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
        "requested_capabilities": [
            "sdk.capability.topics",
            "sdk.capability.topic_subscriptions",
            "sdk.capability.topic_fanout",
            "sdk.capability.telemetry_query",
            "sdk.capability.telemetry_stream",
            "sdk.capability.attachments",
            "sdk.capability.attachment_delete",
            "sdk.capability.attachment_streaming",
            "sdk.capability.markers",
            "sdk.capability.identity_multi",
            "sdk.capability.identity_discovery",
            "sdk.capability.identity_import_export",
            "sdk.capability.identity_hash_resolution",
            "sdk.capability.contact_management",
            "sdk.capability.paper_messages",
            "sdk.capability.remote_commands",
            "sdk.capability.voice_signaling",
            "sdk.capability.group_delivery",
            "sdk.capability.shared_instance_rpc_auth"
        ],
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
fn sdk_conformance_negotiation_release_window_fallback_and_unknown_capability_handling() {
    let harness = RpcHarness::new();

    let fallback_client = harness.client();
    let mut fallback_request = base_start_request();
    fallback_request.supported_contract_versions = vec![4, 3, 2];
    let fallback_handle = fallback_client
        .start(fallback_request)
        .expect("start with future versions should fall back");
    assert_eq!(fallback_handle.active_contract_version, 2);

    let future_only_client = harness.client();
    let mut future_only_request = base_start_request();
    future_only_request.supported_contract_versions = vec![4, 3];
    let future_only_error = future_only_client
        .start(future_only_request)
        .expect_err("future-only contract set must fail");
    assert_eq!(future_only_error.machine_code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");

    let overlap_client = harness.client();
    let mut overlap_request = base_start_request();
    overlap_request.requested_capabilities = vec![
        "sdk.capability.shared_instance_rpc_auth".to_owned(),
        "sdk.capability.future_contract_extension".to_owned(),
    ];
    let overlap_handle = overlap_client
        .start(overlap_request)
        .expect("known capability overlap should succeed even with unknown capability present");
    assert!(
        overlap_handle
            .effective_capabilities
            .iter()
            .any(|capability| capability == "sdk.capability.shared_instance_rpc_auth"),
        "known requested capability must be retained in effective set"
    );
    assert!(
        overlap_handle
            .effective_capabilities
            .iter()
            .all(|capability| capability != "sdk.capability.future_contract_extension"),
        "unknown requested capability must not appear in effective set"
    );
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

#[tokio::test]
async fn sdk_conformance_async_rpc_command_path_matches_sync_contract() {
    let harness = RpcHarness::new();
    let client = harness.client();
    let handle =
        client.start_async(base_start_request()).await.expect("async start should negotiate");
    assert_eq!(handle.active_contract_version, 2);

    let message_id = client
        .send_async(send_request("async-payload", Some("async-idem-key")))
        .await
        .expect("async send");
    let deduped = client
        .send_async(send_request("async-payload", Some("async-idem-key")))
        .await
        .expect("async idempotent send");
    assert_eq!(message_id, deduped, "async idempotency must match sync behavior");

    let status = client.status_async(message_id.clone()).await.expect("async status");
    assert!(status.is_some(), "async status should resolve the sent message");
    let snapshot = client.snapshot_async().await.expect("async snapshot");
    assert_eq!(snapshot.active_contract_version, 2);

    let shutdown =
        client.shutdown_async(lxmf_sdk::ShutdownMode::Graceful).await.expect("async shutdown");
    assert!(shutdown.accepted);
    let second_shutdown = client
        .shutdown_async(lxmf_sdk::ShutdownMode::Graceful)
        .await
        .expect("async shutdown idempotency");
    assert!(second_shutdown.accepted);

    let err = client
        .send_async(send_request("after-stop", None))
        .await
        .expect_err("async send after shutdown must be illegal");
    assert_eq!(err.machine_code, "SDK_RUNTIME_INVALID_STATE");
}

#[test]
fn sdk_conformance_group_send_partial_outcomes() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let result = client
        .send_group(GroupSendRequest::new(
            "source.test",
            vec!["destination.test", "", "destination.test"],
            json!({ "content": "group payload" }),
        ))
        .expect("group send should return outcomes");

    assert_eq!(result.outcomes.len(), 3);
    assert_eq!(
        result.accepted_count + result.deferred_count + result.failed_count,
        result.outcomes.len(),
        "group send counters must match number of outcomes"
    );
    assert!(
        result.outcomes.iter().any(
            |outcome| outcome.reason_code.as_deref() == Some("SDK_VALIDATION_INVALID_ARGUMENT")
        ),
        "group send should classify empty destinations as per-recipient validation failures"
    );
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
fn sdk_conformance_expired_cursor_requires_reset_and_reports_gap() {
    let harness = RpcHarness::new();
    let client = harness.client();
    let mut start_request = base_start_request();
    start_request.config.overflow_policy = OverflowPolicy::DropOldest;
    client.start(start_request).expect("start");

    harness.emit_event("seed_event", json!({ "idx": 1 }));
    let first = client.poll_events(None, 1).expect("initial poll");
    assert_eq!(first.events.len(), 1);
    let stale_cursor = first.next_cursor;

    for idx in 0..EVENT_LOG_OVERFLOW_TRIGGER {
        harness.emit_event("overflow_event", json!({ "idx": idx }));
    }

    let expired = client
        .poll_events(Some(stale_cursor), 1)
        .expect_err("stale cursor outside retained window must expire");
    assert_eq!(expired.machine_code, "SDK_RUNTIME_CURSOR_EXPIRED");

    let degraded = client
        .poll_events(Some(EventCursor("v2:sdk-test-runtime:sdk-events-v2:999999".to_owned())), 1)
        .expect_err("cursored poll after expiry must remain degraded until reset");
    assert_eq!(degraded.machine_code, "SDK_RUNTIME_STREAM_DEGRADED");

    let reset = client.poll_events(None, 8).expect("explicit reset");
    assert!(
        reset.events.iter().any(|event| event.event_type == "StreamGap"),
        "reset after cursor expiry must surface a StreamGap event"
    );
    assert!(reset.dropped_count > 0, "reset should report dropped events");
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
fn sdk_conformance_duplicate_delivery_replay_preserves_event_identity() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    let subscription =
        client.subscribe_events(SubscriptionStart::Head).expect("subscribe from head");
    harness.emit_event("duplicate_probe", json!({ "idx": 1 }));

    let first = client.poll_events(subscription.cursor.clone(), 1).expect("first delivery");
    let replay = client.poll_events(subscription.cursor, 1).expect("replayed delivery");

    assert_eq!(first.events.len(), 1);
    assert_eq!(replay.events.len(), 1);
    assert_eq!(
        first.events[0].event_id, replay.events[0].event_id,
        "at-least-once replay must preserve the event identity for consumer dedupe"
    );
    assert_eq!(
        first.events[0].seq_no, replay.events[0].seq_no,
        "at-least-once replay must preserve event ordering metadata"
    );
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_catches_up_after_stream_close() {
    use tokio_stream::StreamExt;

    let backend = AppStreamConformanceBackend::new(
        vec![sdk_event(1, "live_probe"), sdk_event(2, "live_probe")],
        vec![sdk_event(3, "catchup_probe")],
    );
    let state = Arc::clone(&backend.state);
    let app = lxmf_sdk::app::Client::new(backend);
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    let first = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("first event should arrive")
        .expect("stream should remain open")
        .expect("first event should decode");
    let second = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("second event should arrive")
        .expect("stream should remain open")
        .expect("second event should decode");
    let catchup = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("catch-up event should arrive")
        .expect("stream should remain open")
        .expect("catch-up event should decode");

    assert_eq!((first.metadata.seq_no, second.metadata.seq_no, catchup.metadata.seq_no), (1, 2, 3));
    let cursors = state.poll_cursors.lock().expect("poll cursors mutex");
    assert_eq!(
        cursors.first().and_then(|cursor| cursor.as_ref()).map(|cursor| cursor.0.as_str()),
        Some("v2:conformance-runtime:sdk-events-v2:2"),
        "catch-up poll must resume from the last delivered native event cursor"
    );
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_backpressures_slow_consumers() {
    use tokio_stream::StreamExt;

    let stats = Arc::new(SlowConsumerStats {
        attempted_sends: AtomicUsize::new(0),
        completed_sends: AtomicUsize::new(0),
    });
    let backend = SlowConsumerConformanceBackend::new(3, Arc::clone(&stats));
    let app = lxmf_sdk::app::Client::new(backend);
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        stats.completed_sends.load(Ordering::Relaxed) <= 1,
        "bounded stream should not complete all sends before the consumer starts draining"
    );
    assert!(
        stats.attempted_sends.load(Ordering::Relaxed) >= 2,
        "producer should be blocked on a later send, not idle"
    );

    let first = tokio::time::timeout(Duration::from_secs(1), events.next())
        .await
        .expect("first event should arrive")
        .expect("stream should remain open")
        .expect("first event should decode");
    assert_eq!(first.metadata.seq_no, 1);

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        stats.completed_sends.load(Ordering::Relaxed) >= 2,
        "draining one event should release producer progress"
    );
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_delivers_ordered_typed_events() {
    use tokio_stream::StreamExt;

    let harness = RpcHarness::new();
    let app = lxmf_sdk::app::Client::rpc(harness.endpoint.clone());
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    harness.emit_event("conformance_event", json!({ "idx": 1 }));
    harness.emit_event("conformance_event", json!({ "idx": 2 }));

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    let mut observed = Vec::new();
    while observed.len() < 2 {
        let event = tokio::time::timeout(Duration::from_secs(2), events.next())
            .await
            .expect("event stream should make progress")
            .expect("event stream should remain open")
            .expect("event should decode");
        if event.raw_event_type == "conformance_event" {
            observed.push((
                event.metadata.seq_no,
                event.details.get("idx").and_then(JsonValue::as_u64).expect("idx"),
            ));
        }
    }

    assert_eq!(observed[0].1, 1);
    assert_eq!(observed[1].1, 2);
    assert!(observed[1].0 > observed[0].0, "app event stream must preserve SDK event ordering");
}

#[tokio::test]
async fn sdk_conformance_app_native_event_stream_reports_gap_as_typed_event() {
    use tokio_stream::StreamExt;

    let harness = RpcHarness::new();
    let app = lxmf_sdk::app::Client::rpc(harness.endpoint.clone());
    app.runtime().start(lxmf_sdk::app::Config::desktop_default()).expect("start");

    for idx in 0..EVENT_LOG_OVERFLOW_TRIGGER {
        harness.emit_event("flood", json!({ "idx": idx }));
    }

    let mut events = app
        .events()
        .subscribe(lxmf_sdk::app::SubscriptionStart::Head)
        .expect("subscribe app event stream");
    let event = tokio::time::timeout(Duration::from_secs(2), events.next())
        .await
        .expect("event stream should report overflow")
        .expect("event stream should remain open")
        .expect("event should decode");

    assert!(
        matches!(event.kind, lxmf_sdk::app::EventKind::StreamGapDetected(_)),
        "app event stream must surface stream gaps as typed events"
    );
    let status = app.runtime().status().expect("runtime status");
    assert_eq!(status.state, lxmf_sdk::app::RunState::Degraded);
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

#[test]
fn sdk_conformance_delivery_modes_and_paper_workflows_are_compatible() {
    let harness = RpcHarness::new();
    let client = harness.client();
    client.start(base_start_request()).expect("start");

    for mode in ["direct", "opportunistic", "propagated"] {
        let message_id = format!("mode-{mode}-{}", timestamp_millis());
        let mut send_params = json!({
            "id": message_id,
            "source": "source.test",
            "destination": "destination.test",
            "title": "",
            "content": format!("content-{mode}"),
            "method": mode
        });
        if mode == "propagated" {
            send_params["include_ticket"] = json!(true);
            send_params["try_propagation_on_fail"] = json!(true);
            send_params["stamp_cost"] = json!(8);
        }

        let send_response = harness.rpc_call("send_message_v2", Some(send_params));
        assert!(send_response.error.is_none(), "send_message_v2 should succeed for mode={mode}");

        let trace_response =
            harness.rpc_call("message_delivery_trace", Some(json!({ "message_id": message_id })));
        assert!(
            trace_response.error.is_none(),
            "message_delivery_trace should succeed for mode={mode}"
        );
        let statuses = trace_response
            .result
            .and_then(|value| value.get("transitions").cloned())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|transition| {
                transition.get("status").and_then(JsonValue::as_str).map(str::to_owned)
            })
            .collect::<Vec<_>>();
        assert!(
            statuses.iter().any(|status| status.contains(&format!("sent: {mode}"))),
            "delivery trace should contain sent status for mode={mode}; statuses={statuses:?}"
        );
    }

    let paper_message_id = format!("paper-msg-{}", timestamp_millis());
    let paper_send = harness.rpc_call(
        "send_message_v2",
        Some(json!({
            "id": paper_message_id,
            "source": "source.test",
            "destination": "destination.test",
            "title": "",
            "content": "paper workflow body"
        })),
    );
    assert!(paper_send.error.is_none(), "send_message_v2 should succeed for paper workflow");

    let paper_encode =
        harness.rpc_call("sdk_paper_encode_v2", Some(json!({ "message_id": paper_message_id })));
    assert!(paper_encode.error.is_none(), "sdk_paper_encode_v2 should succeed");
    let uri = paper_encode
        .result
        .and_then(|value| value.get("envelope").cloned())
        .and_then(|value| value.get("uri").cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
        .expect("paper encode response must include envelope uri");

    let paper_decode = harness.rpc_call("sdk_paper_decode_v2", Some(json!({ "uri": uri })));
    assert!(paper_decode.error.is_none(), "sdk_paper_decode_v2 should succeed");
    assert_eq!(
        paper_decode
            .result
            .and_then(|value| value.get("accepted").cloned())
            .and_then(|value| value.as_bool()),
        Some(true),
        "paper decode result must report accepted=true"
    );
}
