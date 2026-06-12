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
