use super::*;
use crate::request_policy::RequestPolicy;
use axum::http::{HeaderValue, Version};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn state() -> Arc<AppState> {
    let (events, _) = broadcast::channel(16);
    Arc::new(AppState {
        daemon: Arc::new(RpcDaemon::test_instance()),
        local: Arc::new(RwLock::new(LocalState::default())),
        events,
        persist: None,
        config: None,
    })
}

fn bound_router(bind: &str) -> Router {
    build_router(state(), None, RequestPolicy::for_bind(bind.parse().expect("bind")))
}

fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::empty()).expect("request")
}

async fn response_status(app: &Router, request: Request<Body>) -> StatusCode {
    app.clone().oneshot(request).await.expect("response").status()
}

#[tokio::test]
async fn cross_origin_http_is_denied_before_config_changes() {
    let state = state();
    let app = build_router(
        state.clone(),
        None,
        RequestPolicy::for_bind("127.0.0.1:8000".parse().expect("bind")),
    );
    let request = Request::builder()
        .method("PATCH")
        .uri("/api/v1/config")
        .header("host", "127.0.0.1:8000")
        .header("origin", "https://evil.example")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"config":{"theme":"dark"}}"#))
        .expect("request");
    assert_eq!(response_status(&app, request).await, StatusCode::FORBIDDEN);
    assert!(state.local.read().await.config.is_empty());
}

#[tokio::test]
async fn rebinding_and_forwarded_host_spoofing_are_denied() {
    let app = bound_router("127.0.0.1:8000");
    for headers in [
        vec![("host", "evil.example:8000")],
        vec![("host", "evil.example:8000"), ("origin", "http://evil.example:8000")],
        vec![("host", "evil.example:8000"), ("x-forwarded-host", "localhost:8000")],
        vec![("host", "evil.example:8000"), ("forwarded", "host=localhost:8000")],
        vec![("x-forwarded-host", "127.0.0.1:8000")],
    ] {
        assert_eq!(
            response_status(&app, request("GET", "/api/v1/lxmf/conversations", &headers)).await,
            StatusCode::FORBIDDEN,
            "{headers:?}",
        );
    }
}

#[tokio::test]
async fn malformed_duplicate_and_mismatched_headers_are_denied() {
    let app = bound_router("127.0.0.1:8000");
    let hosts = [
        "",
        "localhost",
        "localhost:8001",
        "localhost:",
        "localhost:65536",
        "localhost:+8000",
        "127.0.0.2:8000",
        "[::1]:8000",
        "192.168.1.1:8000",
        "localhost.evil.example:8000",
        "localhost.:8000",
        "user@localhost:8000",
        "localhost:8000/path",
        "localhost:8000#fragment",
        "localhost:8000,evil.example:8000",
        "127.1:8000",
        "2130706433:8000",
        "localhost%00:8000",
        "::1",
        "::1:8000",
        "[127.0.0.1]:8000",
    ];
    for host in hosts {
        assert_eq!(
            response_status(&app, request("GET", "/api/v1/config", &[("host", host)])).await,
            StatusCode::FORBIDDEN,
            "Host: {host}",
        );
    }
    for origin in [
        "",
        "null",
        "https://localhost:8000",
        "ws://localhost:8000",
        "http://localhost:8001",
        "http://127.0.0.1:8000",
        "http://localhost",
        "http://localhost:8000/",
        "http://localhost:8000?query",
        "http://localhost:8000#fragment",
        "http://user@localhost:8000",
        "http://localhost:8000 https://evil.example",
        "http://localhost:8000,https://evil.example",
    ] {
        assert_eq!(
            response_status(
                &app,
                request("GET", "/api/v1/config", &[("host", "localhost:8000"), ("origin", origin)])
            )
            .await,
            StatusCode::FORBIDDEN,
            "Origin: {origin}",
        );
    }
    for name in ["host", "origin", "sec-fetch-site"] {
        let mut req = request("GET", "/api/v1/config", &[("host", "localhost:8000")]);
        let value = match name {
            "host" => "localhost:8000",
            "origin" => "http://localhost:8000",
            _ => "same-origin",
        };
        if name != "host" {
            req.headers_mut().append(name, HeaderValue::from_static(value));
        }
        req.headers_mut().append(name, HeaderValue::from_static(value));
        assert_eq!(response_status(&app, req).await, StatusCode::FORBIDDEN, "duplicate {name}");

        let mut req = request("GET", "/api/v1/config", &[("host", "localhost:8000")]);
        req.headers_mut().insert(name, HeaderValue::from_bytes(&[0xff]).expect("opaque value"));
        assert_eq!(response_status(&app, req).await, StatusCode::FORBIDDEN, "invalid {name}");
    }
    assert_eq!(
        response_status(&app, request("GET", "/api/v1/config", &[])).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn originless_cross_site_browser_gets_and_forms_are_denied() {
    let app = bound_router("127.0.0.1:8000");
    for site in ["cross-site", "same-site", "garbage", "same-origin, cross-site", ""] {
        for (method, path) in [
            ("GET", "/api/v1/lxmf/propagation-node/sync"),
            ("GET", "/api/v1/lxmf/conversations/0123456789abcdef0123456789abcdef/mark-as-read"),
            ("POST", "/api/v1/reticulum/enable-transport"),
            ("POST", "/api/v1/reticulum/interfaces/export"),
        ] {
            assert_eq!(
                response_status(
                    &app,
                    request(method, path, &[("host", "127.0.0.1:8000"), ("sec-fetch-site", site)])
                )
                .await,
                StatusCode::FORBIDDEN,
                "{site} {method} {path}",
            );
        }
    }
}

#[tokio::test]
async fn same_origin_loopback_variants_and_non_browser_requests_succeed() {
    for (bind, host) in [
        ("127.0.0.1:8000", "127.0.0.1:8000"),
        ("127.0.0.1:8000", "localhost:8000"),
        ("127.0.0.1:8000", "LOCALHOST:8000"),
        ("127.0.0.2:8000", "127.0.0.2:8000"),
        ("[::1]:8000", "[::1]:8000"),
        ("[::1]:8000", "[0:0:0:0:0:0:0:1]:8000"),
        ("[::1]:8000", "localhost:8000"),
        ("127.0.0.1:80", "127.0.0.1"),
        ("[::1]:80", "[::1]"),
    ] {
        let app = bound_router(bind);
        let origin = format!("http://{host}");
        for browser_headers in [
            vec![],
            vec![("origin", origin.as_str()), ("sec-fetch-site", "same-origin")],
            vec![("sec-fetch-site", "none")],
        ] {
            let mut headers = vec![("host", host)];
            headers.extend(browser_headers);
            assert_eq!(
                response_status(&app, request("GET", "/api/v1/config", &headers)).await,
                StatusCode::OK,
                "{bind} {headers:?}"
            );
        }
    }
    let app = bound_router("127.0.0.1:80");
    assert_eq!(
        response_status(
            &app,
            request(
                "GET",
                "/api/v1/config",
                &[("host", "localhost"), ("origin", "http://localhost:80")]
            )
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn same_origin_http_patch_reaches_handler() {
    let app = bound_router("127.0.0.1:8000");
    let request = Request::builder()
        .method("PATCH")
        .uri("/api/v1/config")
        .header("host", "localhost:8000")
        .header("origin", "http://localhost:8000")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"config":{"theme":"dark"}}"#))
        .expect("request");
    let response = app.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.expect("body");
    let config: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(config["config"]["theme"], "dark");
}

#[tokio::test]
async fn public_router_enforces_origin_and_host_without_listener_knowledge() {
    let app = router(Arc::new(RpcDaemon::test_instance()), None);
    for (host, origin, expected) in [
        ("localhost:9123", "http://localhost:9123", StatusCode::OK),
        ("[::1]:1234", "http://[::1]:1234", StatusCode::OK),
        ("evil.example:9123", "http://evil.example:9123", StatusCode::FORBIDDEN),
        ("localhost:9123", "https://evil.example", StatusCode::FORBIDDEN),
    ] {
        assert_eq!(
            response_status(
                &app,
                request("GET", "/api/v1/config", &[("host", host), ("origin", origin)])
            )
            .await,
            expected
        );
    }
}

#[tokio::test]
async fn static_assets_and_fallback_receive_the_same_policy() {
    let temp = tempfile::tempdir().expect("assets");
    std::fs::write(temp.path().join("index.html"), "MeshChat").expect("index");
    let app = build_router(
        state(),
        Some(temp.path().to_path_buf()),
        RequestPolicy::for_bind("127.0.0.1:8000".parse().expect("bind")),
    );
    for path in ["/", "/index.html", "/missing"] {
        assert_eq!(
            response_status(&app, request("GET", path, &[("host", "evil.example:8000")])).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            response_status(
                &app,
                request(
                    "GET",
                    path,
                    &[("host", "localhost:8000"), ("origin", "https://evil.example")]
                )
            )
            .await,
            StatusCode::FORBIDDEN
        );
    }
    for headers in [
        vec![("host", "localhost:8000")],
        vec![("host", "localhost:8000"), ("sec-fetch-site", "none")],
        vec![("host", "localhost:8000"), ("origin", "http://localhost:8000")],
    ] {
        assert_eq!(response_status(&app, request("GET", "/", &headers)).await, StatusCode::OK);
    }
}

#[tokio::test]
async fn http2_authority_is_validated_without_requiring_a_host_header() {
    let app = bound_router("127.0.0.1:8000");
    for (uri, host, expected) in [
        ("http://localhost:8000/api/v1/config", None, StatusCode::OK),
        ("http://evil.example:8000/api/v1/config", None, StatusCode::FORBIDDEN),
        ("http://localhost:8001/api/v1/config", None, StatusCode::FORBIDDEN),
        ("https://localhost:8000/api/v1/config", None, StatusCode::FORBIDDEN),
        ("http://evil.example:8000/api/v1/config", Some("localhost:8000"), StatusCode::FORBIDDEN),
    ] {
        let mut req = request("GET", uri, &[]);
        *req.version_mut() = Version::HTTP_2;
        if let Some(host) = host {
            req.headers_mut().insert("host", host.parse().expect("host"));
        }
        assert_eq!(response_status(&app, req).await, expected, "{uri} {host:?}");
    }
    for uri in ["http://evil.example:8000/api/v1/config", "https://localhost:8000/api/v1/config"] {
        assert_eq!(
            response_status(&app, request("GET", uri, &[("host", "localhost:8000")])).await,
            StatusCode::FORBIDDEN
        );
    }
}

async fn websocket_handshake(bind: SocketAddr, host: &str, origin: Option<&str>) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut stream = tokio::net::TcpStream::connect(bind).await.expect("connect");
        let origin = origin.map(|origin| format!("Origin: {origin}\r\n")).unwrap_or_default();
        let handshake = format!("GET /ws HTTP/1.1\r\nHost: {host}\r\n{origin}Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n");
        stream.write_all(handshake.as_bytes()).await.expect("handshake");
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(stream.read_u8().await.expect("response headers"));
            assert!(headers.len() < 8192);
        }
        String::from_utf8(headers).expect("header text")
    }).await.expect("handshake timeout")
}

#[tokio::test]
async fn websocket_upgrades_only_for_allowed_origins_on_a_live_listener() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let bind = listener.local_addr().expect("actual port");
    let app = build_router(state(), None, RequestPolicy::for_bind(bind));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("server") });
    let host = bind.to_string();
    for origin in [Some("https://evil.example"), Some("null"), Some("https://localhost:8000")] {
        let response = websocket_handshake(bind, &host, origin).await;
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    }
    let response = websocket_handshake(bind, &format!("evil.example:{}", bind.port()), None).await;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    let origin = format!("http://{host}");
    for origin in [Some(origin.as_str()), None] {
        let response = websocket_handshake(bind, &host, origin).await;
        assert!(response.starts_with("HTTP/1.1 101"), "{response}");
    }
    server.abort();
    assert!(server.await.expect_err("aborted server").is_cancelled());
}
