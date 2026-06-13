    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_retries_transient_reconnect_failure_after_disconnect() {
        use tokio::io::AsyncWriteExt as _;

        let first_listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind first listener");
        let addr = first_listener.local_addr().expect("first listener address");
        let authority = addr.to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let (mut first_socket, _) = first_listener.accept().await.expect("accept first stream");
        let first_request = read_event_stream_request(&mut first_socket).await;
        assert!(first_request.starts_with("GET /events/stream HTTP/1.1"));
        first_socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write first response header");
        let first_frame = codec::encode_frame(&test_sdk_event(1)).expect("encode first event");
        first_socket.write_all(&first_frame).await.expect("write first event");
        first_socket.shutdown().await.expect("close first stream");
        drop(first_listener);

        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("first event should arrive before restart")
            .expect("stream should stay open")
            .expect("first event should decode");
        assert_eq!(first.seq_no, 1);

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await.is_err(),
            "transient reconnect refusal must not terminate the app stream"
        );

        let second_listener =
            tokio::net::TcpListener::bind(addr).await.expect("rebind listener after restart");
        let second_request = accept_event_stream_request(&second_listener, test_sdk_event(2)).await;
        assert!(second_request
            .starts_with("GET /events/stream?cursor=v2:rt-test:sdk-events-v2:1 HTTP/1.1"));

        let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("second event should arrive after listener returns")
            .expect("stream should stay open")
            .expect("second event should decode");
        assert_eq!(second.seq_no, 2);

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_recovers_after_malformed_frame() {
        use tokio::io::AsyncWriteExt as _;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let (mut first_socket, _) = listener.accept().await.expect("accept first stream");
        let first_request = read_event_stream_request(&mut first_socket).await;
        assert!(first_request.starts_with("GET /events/stream HTTP/1.1"));
        first_socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write first response header");
        let first_frame = codec::encode_frame(&test_sdk_event(1)).expect("encode first event");
        first_socket.write_all(&first_frame).await.expect("write first event");
        first_socket.write_all(&[0, 0, 0, 1, 0xc1]).await.expect("write malformed event frame");
        first_socket.shutdown().await.expect("close malformed stream");

        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("first event should arrive before malformed frame")
            .expect("stream should stay open")
            .expect("first event should decode");
        assert_eq!(first.seq_no, 1);

        let second_request = accept_event_stream_request(&listener, test_sdk_event(2)).await;
        assert!(second_request
            .starts_with("GET /events/stream?cursor=v2:rt-test:sdk-events-v2:1 HTTP/1.1"));

        let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("second event should arrive after reconnect")
            .expect("stream should stay open")
            .expect("second event should decode");
        assert_eq!(second.seq_no, 2);

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await.is_err(),
            "malformed frame must not be emitted as an app-facing event or error"
        );

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_deduplicates_replayed_cursor_event() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let first_request = accept_event_stream_request(&listener, test_sdk_event(1)).await;
        assert!(first_request.starts_with("GET /events/stream HTTP/1.1"));

        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("first event should arrive")
            .expect("stream should stay open")
            .expect("first event should decode");
        assert_eq!(first.seq_no, 1);

        let replay_request = accept_event_stream_request_with_events(
            &listener,
            [test_sdk_event(1), test_sdk_event(2)],
        )
        .await;
        assert!(replay_request
            .starts_with("GET /events/stream?cursor=v2:rt-test:sdk-events-v2:1 HTTP/1.1"));

        let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("second event should arrive after replay")
            .expect("stream should stay open")
            .expect("second event should decode");
        assert_eq!(second.seq_no, 2);

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await.is_err(),
            "replayed cursor event must not be delivered twice"
        );

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_surfaces_framed_runtime_error() {
        use tokio::io::AsyncWriteExt as _;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let (mut socket, _) = listener.accept().await.expect("accept stream");
        let request = read_event_stream_request(&mut socket).await;
        assert!(request.starts_with("GET /events/stream HTTP/1.1"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        let rpc_response = RpcResponse {
            id: 0,
            result: None,
            error: Some(RpcError::new(
                code::RUNTIME_INVALID_CURSOR,
                "event stream cursor is invalid",
            )),
        };
        let frame = codec::encode_frame(&rpc_response).expect("encode rpc error frame");
        socket.write_all(&frame).await.expect("write rpc error frame");
        socket.shutdown().await.expect("close error stream");

        let err = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("runtime error should arrive")
            .expect("stream should emit error")
            .expect_err("framed rpc error should surface to app");
        assert_eq!(err.machine_code, code::RUNTIME_INVALID_CURSOR);
        assert_eq!(err.category, ErrorCategory::Runtime);

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(350), listener.accept())
                .await
                .is_err(),
            "terminal framed runtime errors must not reconnect in a tight loop"
        );

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_backpressures_when_consumer_is_slow() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(1);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let first_request = accept_event_stream_request_with_events(
            &listener,
            [test_sdk_event(1), test_sdk_event(2), test_sdk_event(3)],
        )
        .await;
        assert!(first_request.starts_with("GET /events/stream HTTP/1.1"));

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
                .await
                .is_err(),
            "bounded channel should stall the reader before it reconnects"
        );

        let first = rx.recv().await.expect("first queued event").expect("first event");
        assert_eq!(first.seq_no, 1);
        let second = rx.recv().await.expect("second queued event").expect("second event");
        assert_eq!(second.seq_no, 2);

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_task_aborts_when_receiver_stream_is_dropped() {
        struct DropNotify(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for DropNotify {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }

        let (_tx, rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(1);
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notify = DropNotify(Some(dropped_tx));
            std::future::pending::<()>().await;
        });

        let stream = AbortOnDropStream::new(ReceiverStream::new(rx), task);
        tokio::task::yield_now().await;
        drop(stream);

        tokio::time::timeout(std::time::Duration::from_secs(1), dropped_rx)
            .await
            .expect("background stream task should abort on drop")
            .expect("drop notification should be delivered");
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_rejects_oversized_frame_before_allocation() {
        let len = (RPC_EVENT_STREAM_MAX_FRAME_BYTES as u32) + 1;
        let bytes = len.to_be_bytes();
        let mut stream = &bytes[..];

        let err = read_rpc_http_event_frame(&mut stream)
            .await
            .expect_err("oversized frame should fail before payload allocation");
        assert_eq!(err.category, ErrorCategory::Transport);
        assert!(err.message.contains("event stream frame exceeded"));
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_rejection_preserves_typed_rpc_error() {
        let rpc_response = RpcResponse {
            id: 0,
            result: None,
            error: Some(RpcError::new(
                "SDK_SECURITY_AUTH_REQUIRED",
                "event stream requires authentication",
            )),
        };
        let body = codec::encode_frame(&rpc_response).expect("encode rpc error frame");
        let response_header = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut response = response_header.into_bytes();
        response.extend_from_slice(&body);
        let mut stream = &response[..];

        let err = read_rpc_http_event_header(&mut stream)
            .await
            .expect_err("event stream rejection should surface typed error");
        assert_eq!(err.machine_code, code::SECURITY_AUTH_REQUIRED);
        assert_eq!(err.category, ErrorCategory::Security);
        assert!(err.is_user_actionable);
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_rejection_rejects_oversized_body_before_allocation() {
        let response_header = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
            RPC_EVENT_STREAM_MAX_FRAME_BYTES + 1
        );
        let mut stream = response_header.as_bytes();

        let err = read_rpc_http_event_header(&mut stream)
            .await
            .expect_err("oversized rejection body should fail before allocation");
        assert_eq!(err.category, ErrorCategory::Transport);
        assert!(err.message.contains("event stream rejection body exceeded"));
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_frame_preserves_framed_rpc_error() {
        let rpc_response = RpcResponse {
            id: 0,
            result: None,
            error: Some(RpcError::new(
                "SDK_SECURITY_AUTH_REQUIRED",
                "event stream requires authentication",
            )),
        };
        let frame = codec::encode_frame(&rpc_response).expect("encode rpc error frame");
        let mut stream = &frame[..];

        let err = read_rpc_http_event_frame(&mut stream)
            .await
            .expect_err("framed rpc error should surface typed sdk error");
        assert_eq!(err.machine_code, code::SECURITY_AUTH_REQUIRED);
        assert_eq!(err.category, ErrorCategory::Security);
        assert!(err.is_user_actionable);
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn open_event_stream_uses_native_stream_for_mtls_auth() {
        let client = RpcBackendClient::new("127.0.0.1:9");
        {
            let mut auth = client.session_auth.write().expect("session auth");
            *auth = SessionAuth::Mtls {
                ca_bundle_path: "/definitely/missing/ca.pem".to_string(),
                client_cert_path: None,
                client_key_path: None,
            };
        }

        let stream = client
            .open_event_stream_impl(&EventSubscription {
                start: SubscriptionStart::Head,
                cursor: None,
            })
            .expect("stream creation should not fall back for mtls");

        assert!(stream.is_some(), "mTLS sessions should use the native stream connector");
    }

    #[test]
    fn rpc_response_reader_rejects_oversized_sync_response() {
        let response = vec![0_u8; RPC_HTTP_RESPONSE_MAX_BYTES + 1];
        let mut cursor = std::io::Cursor::new(response);

        let err = RpcBackendClient::read_http_response_to_end(&mut cursor)
            .expect_err("oversized response should fail");

        assert_eq!(err.category, ErrorCategory::Transport);
        assert!(err.message.contains("rpc response exceeded"));
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn rpc_response_reader_rejects_oversized_async_response() {
        let response = vec![0_u8; RPC_HTTP_RESPONSE_MAX_BYTES + 1];
        let mut cursor = std::io::Cursor::new(response);

        let err = RpcBackendClient::read_http_response_to_end_async(&mut cursor)
            .await
            .expect_err("oversized response should fail");

        assert_eq!(err.category, ErrorCategory::Transport);
        assert!(err.message.contains("rpc response exceeded"));
    }

    #[test]
    fn zeroize_header_values_clears_sensitive_header_contents() {
        let mut headers = vec![
            ("Authorization".to_string(), "Bearer super-secret-token".to_string()),
            ("X-Correlation-Id".to_string(), "trace-123".to_string()),
        ];

        RpcBackendClient::zeroize_header_values(headers.as_mut_slice());

        assert!(headers.iter().all(|(_, value)| value.is_empty()));
    }

    #[test]
    fn mtls_for_session_auth_returns_mtls_paths_only() {
        let mtls_auth = SessionAuth::Mtls {
            ca_bundle_path: "/tmp/ca.pem".to_string(),
            client_cert_path: Some("/tmp/client.pem".to_string()),
            client_key_path: Some("/tmp/client.key".to_string()),
        };
        let extracted =
            RpcBackendClient::mtls_for_session_auth(&mtls_auth).expect("mtls config expected");
        assert_eq!(extracted.ca_bundle_path, "/tmp/ca.pem");
        assert_eq!(extracted.client_cert_path.as_deref(), Some("/tmp/client.pem"));
        assert_eq!(extracted.client_key_path.as_deref(), Some("/tmp/client.key"));

        assert!(RpcBackendClient::mtls_for_session_auth(&SessionAuth::LocalTrusted).is_none());
        assert!(RpcBackendClient::mtls_for_session_auth(&SessionAuth::Token {
            issuer: "issuer".to_string(),
            audience: "audience".to_string(),
            shared_secret: Zeroizing::new("secret".to_string()),
            ttl_secs: 60,
        })
        .is_none());
    }
