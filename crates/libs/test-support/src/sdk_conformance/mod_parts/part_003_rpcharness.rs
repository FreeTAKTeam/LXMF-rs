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
