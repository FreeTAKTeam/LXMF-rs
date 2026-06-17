    use super::*;

    #[cfg(feature = "sdk-async")]
    fn test_sdk_event(seq_no: u64) -> SdkEvent {
        SdkEvent {
            event_id: format!("evt-{seq_no}"),
            runtime_id: "rt-test".to_string(),
            stream_id: "sdk-events-v2".to_string(),
            seq_no,
            contract_version: 2,
            ts_ms: seq_no,
            event_type: "RuntimeStateChanged".to_string(),
            severity: Severity::Info,
            source_component: "transport-test".to_string(),
            operation_id: None,
            message_id: None,
            peer_id: None,
            correlation_id: None,
            trace_id: None,
            payload: serde_json::json!({ "to": "running" }),
            extensions: BTreeMap::new(),
        }
    }

    #[cfg(feature = "sdk-async")]
    async fn read_event_stream_request(socket: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt as _;

        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.expect("read event stream request");
            request.push(byte[0]);
        }
        String::from_utf8(request).expect("request should be valid utf8")
    }

    #[cfg(feature = "sdk-async")]
    async fn accept_event_stream_request(
        listener: &tokio::net::TcpListener,
        event: SdkEvent,
    ) -> String {
        use tokio::io::AsyncWriteExt as _;

        let (mut socket, _) = listener.accept().await.expect("accept event stream client");
        let request = read_event_stream_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        let frame = codec::encode_frame(&event).expect("encode event frame");
        socket.write_all(&frame).await.expect("write event frame");
        request
    }

    #[cfg(feature = "sdk-async")]
    async fn accept_event_stream_request_with_events(
        listener: &tokio::net::TcpListener,
        events: impl IntoIterator<Item = SdkEvent>,
    ) -> String {
        use tokio::io::AsyncWriteExt as _;

        let (mut socket, _) = listener.accept().await.expect("accept event stream client");
        let request = read_event_stream_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        for event in events {
            let frame = codec::encode_frame(&event).expect("encode event frame");
            socket.write_all(&frame).await.expect("write event frame");
        }
        request
    }

    #[cfg(feature = "sdk-async")]
    fn request_header_value<'a>(request: &'a str, name: &str) -> Option<&'a str> {
        let prefix = format!("{name}:");
        request.lines().find_map(|line| line.strip_prefix(prefix.as_str()).map(str::trim))
    }

    #[test]
    fn rpc_endpoint_accepts_tcp_scheme_for_http_rpc_compatibility() {
        let endpoint = RpcBackendClient::parse_endpoint("tcp://127.0.0.1:37428/rpc")
            .expect("tcp scheme should be accepted");

        match endpoint {
            RpcEndpoint::Tcp(authority) => assert_eq!(authority, "127.0.0.1:37428"),
            RpcEndpoint::Unix(_) => panic!("tcp scheme must not parse as unix endpoint"),
        }
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn call_rpc_async_uses_async_http_post_transport() {
        use rns_rpc::rpc::{RpcRequest, RpcResponse};
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept async rpc client");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.expect("read header byte");
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request.clone()).expect("headers utf8");
            assert!(headers.starts_with("POST /rpc HTTP/1.1"));
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            let mut body = vec![0_u8; content_length];
            socket.read_exact(&mut body).await.expect("read rpc body");
            let rpc_request =
                codec::decode_frame::<RpcRequest>(&body).expect("decode async rpc request");
            assert_eq!(rpc_request.method, "probe_async");

            let response = RpcResponse {
                id: rpc_request.id,
                result: Some(serde_json::json!({ "ok": true })),
                error: None,
            };
            let response_frame = codec::encode_frame(&response).expect("encode response");
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
                response_frame.len()
            );
            socket.write_all(http_response.as_bytes()).await.expect("write response header");
            socket.write_all(&response_frame).await.expect("write response body");
            socket.shutdown().await.expect("shutdown server response");
        });

        let client = RpcBackendClient::new(authority);
        let result = client
            .call_rpc_async("probe_async", Some(serde_json::json!({ "value": 7 })))
            .await
            .expect("async rpc call");
        assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(true));
        server.await.expect("server task");
    }

    #[cfg(all(feature = "sdk-async", unix))]
    fn test_unix_socket_path(label: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "lxmf-sdk-{label}-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("unix epoch")
                .as_nanos()
        ));
        path
    }

    #[cfg(all(feature = "sdk-async", unix))]
    #[tokio::test]
    async fn call_rpc_async_supports_unix_socket_endpoint() {
        use rns_rpc::rpc::{RpcRequest, RpcResponse};
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let path = test_unix_socket_path("rpc");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind unix listener");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept unix rpc client");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.expect("read header byte");
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request.clone()).expect("headers utf8");
            assert!(headers.starts_with("POST /rpc HTTP/1.1"));
            assert!(headers.contains("Host: localhost\r\n"));
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            let mut body = vec![0_u8; content_length];
            socket.read_exact(&mut body).await.expect("read rpc body");
            let rpc_request =
                codec::decode_frame::<RpcRequest>(&body).expect("decode async rpc request");
            assert_eq!(rpc_request.method, "probe_unix_async");

            let response = RpcResponse {
                id: rpc_request.id,
                result: Some(serde_json::json!({ "ok": true })),
                error: None,
            };
            let response_frame = codec::encode_frame(&response).expect("encode response");
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
                response_frame.len()
            );
            socket.write_all(http_response.as_bytes()).await.expect("write response header");
            socket.write_all(&response_frame).await.expect("write response body");
            socket.shutdown().await.expect("shutdown server response");
        });

        let client = RpcBackendClient::new(format!("unix:{}", path.display()));
        let result = client
            .call_rpc_async("probe_unix_async", Some(serde_json::json!({ "value": 7 })))
            .await
            .expect("async unix rpc call");
        assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(true));
        server.await.expect("server task");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(all(feature = "sdk-async", unix))]
    #[tokio::test]
    async fn native_event_stream_supports_unix_socket_endpoint() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let path = test_unix_socket_path("events");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind unix listener");
        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = format!("unix:{}", path.display());
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let (mut socket, _) = listener.accept().await.expect("accept unix event stream client");
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.expect("read event stream request");
            request.push(byte[0]);
        }
        let request = String::from_utf8(request).expect("event stream request utf8");
        assert!(request.starts_with("GET /events/stream HTTP/1.1"));
        assert!(request.contains("Host: localhost\r\n"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        let frame = codec::encode_frame(&test_sdk_event(1)).expect("encode event frame");
        socket.write_all(&frame).await.expect("write event frame");

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("unix event stream should deliver event")
            .expect("stream should stay open")
            .expect("event should decode");
        assert_eq!(event.seq_no, 1);

        client_task.abort();
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_reconnects_with_last_event_cursor() {
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

        let second_request = accept_event_stream_request(&listener, test_sdk_event(2)).await;
        assert!(second_request
            .starts_with("GET /events/stream?cursor=v2:rt-test:sdk-events-v2:1 HTTP/1.1"));

        let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("second event should arrive")
            .expect("stream should stay open")
            .expect("second event should decode");
        assert_eq!(second.seq_no, 2);

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_initial_connect_failure_surfaces_transport_error() {
        let authority = "127.0.0.1:0".to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(authority, EventStreamRequestAuth::LocalTrusted, None, tx)
                .await;
        });

        let err = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("initial connection error should arrive")
            .expect("stream should emit initial connection error")
            .expect_err("initial connection failure should be app-facing");
        assert_eq!(err.category, ErrorCategory::Transport);

        client_task.await.expect("stream task should finish after initial connection failure");
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_refreshes_token_auth_on_reconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(
                endpoint,
                EventStreamRequestAuth::Token {
                    issuer: "test-issuer".to_string(),
                    audience: "test-audience".to_string(),
                    shared_secret: Zeroizing::new("test-secret".to_string()),
                    ttl_secs: 60,
                    stream_id: 42,
                    next_jti: 0,
                },
                None,
                tx,
            )
            .await;
        });

        let first_request = accept_event_stream_request(&listener, test_sdk_event(1)).await;
        let first_auth =
            request_header_value(&first_request, "Authorization").expect("first auth header");
        assert!(first_auth.contains("jti=sdk-stream-jti-42-0"));

        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("first event should arrive")
            .expect("stream should stay open")
            .expect("first event should decode");
        assert_eq!(first.seq_no, 1);

        let second_request = accept_event_stream_request(&listener, test_sdk_event(2)).await;
        let second_auth =
            request_header_value(&second_request, "Authorization").expect("second auth header");
        assert!(second_auth.contains("jti=sdk-stream-jti-42-1"));
        assert_ne!(first_auth, second_auth, "reconnect must not replay the first token jti");

        let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("second event should arrive")
            .expect("stream should stay open")
            .expect("second event should decode");
        assert_eq!(second.seq_no, 2);

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_reconnects_after_mid_frame_disconnect() {
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
        let partial_second_frame =
            codec::encode_frame(&test_sdk_event(2)).expect("encode second event");
        first_socket
            .write_all(&partial_second_frame[..partial_second_frame.len().min(7)])
            .await
            .expect("write partial second event");
        first_socket.shutdown().await.expect("close mid-frame stream");

        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("first event should arrive before reconnect")
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
            "partial mid-frame event must not be emitted as a duplicate or error"
        );

        client_task.abort();
    }
