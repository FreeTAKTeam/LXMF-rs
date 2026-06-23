#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::{RpcDaemon, RpcRequest};

    fn parse_status_line(response: &[u8]) -> &str {
        std::str::from_utf8(response).expect("utf8 response").lines().next().expect("status line")
    }

    fn parse_json_body(response: &[u8]) -> serde_json::Value {
        let header_end = find_header_end(response).expect("header end");
        let body = &response[header_end + HEADER_END.len()..];
        serde_json::from_slice(body).expect("json body")
    }

    fn metric_counter(snapshot: &serde_json::Value, key: &str) -> u64 {
        snapshot
            .get("counters")
            .and_then(|counters| counters.get(key))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    }

    #[test]
    fn event_stream_request_helpers_parse_live_stream_route() {
        let request = b"GET /events/stream HTTP/1.1\r\nHost: localhost\r\nX-Test: yes\r\n\r\n";
        let (method, path, headers) = request_method_path_headers(request).expect("parse request");

        assert_eq!(method, "GET");
        assert_eq!(path, "/events/stream");
        assert!(headers.iter().any(|(name, value)| name == "X-Test" && value == "yes"));

        let header = streaming_event_response_header();
        let header_text = std::str::from_utf8(&header).expect("utf8 header");
        assert!(header_text.starts_with("HTTP/1.1 200 OK"));
        assert!(header_text.contains("Content-Type: application/msgpack"));
        assert!(!header_text.contains("Content-Length"));
    }

    #[test]
    fn health_endpoints_return_http_200_with_json_status() {
        let daemon = RpcDaemon::test_instance();
        for path in ["/healthz", "/readyz", "/livez"] {
            let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n");
            let response = handle_http_request_with_peer(
                &daemon,
                request.as_bytes(),
                Some("127.0.0.1:1".parse().expect("socket")),
            )
            .expect("health endpoint response");
            assert_eq!(parse_status_line(&response), "HTTP/1.1 200 OK");
            let body = parse_json_body(&response);
            assert_eq!(body["ok"], json!(true));
            assert_eq!(body["service"], json!("reticulumd-rpc"));
        }
    }

    #[test]
    fn metrics_endpoint_reports_sdk_flow_counters_and_histograms() {
        let daemon = RpcDaemon::test_instance();
        let _send = daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "sdk_send_v2".to_string(),
                params: Some(json!({
                    "id": "metrics-send-1",
                    "source": "source-a",
                    "destination": "dest-a",
                    "title": "metrics",
                    "content": "metrics payload",
                    "method": "direct",
                })),
            })
            .expect("send response");
        let _poll = daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "sdk_poll_events_v2".to_string(),
                params: Some(json!({
                    "cursor": null,
                    "max": 16,
                })),
            })
            .expect("poll response");

        let request = b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let response = handle_http_request_with_peer(
            &daemon,
            request,
            Some("127.0.0.1:7".parse().expect("socket")),
        )
        .expect("metrics endpoint response");
        assert_eq!(parse_status_line(&response), "HTTP/1.1 200 OK");

        let body = parse_json_body(&response);
        assert!(metric_counter(&body, "sdk_send_total") >= 1);
        assert!(metric_counter(&body, "sdk_send_success_total") >= 1);
        assert!(metric_counter(&body, "sdk_poll_total") >= 1);
        assert!(metric_counter(&body, "sdk_poll_events_total") >= 1);
        assert!(metric_counter(&body, "http_requests_total") >= 1);
        assert!(
            body.get("rpc_requests_by_method")
                .and_then(|value| value.get("sdk_send_v2"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                >= 1
        );
        assert!(
            body.get("histograms")
                .and_then(|value| value.get("sdk_send_latency_ms"))
                .and_then(|value| value.get("count"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                >= 1
        );
        assert!(
            body.get("histograms")
                .and_then(|value| value.get("sdk_poll_latency_ms"))
                .and_then(|value| value.get("count"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                >= 1
        );
    }

    #[test]
    fn metrics_capture_auth_failures_for_remote_local_only_requests() {
        let daemon = RpcDaemon::test_instance();
        let request = b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let _response = handle_http_request_with_peer(
            &daemon,
            request,
            Some("203.0.113.9:1442".parse().expect("socket")),
        )
        .expect("response");
        let snapshot = daemon.metrics_snapshot();
        assert!(metric_counter(&snapshot, "sdk_auth_failures_total") >= 1);
        assert!(
            snapshot
                .get("histograms")
                .and_then(|value| value.get("sdk_auth_latency_ms"))
                .and_then(|value| value.get("count"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                >= 1
        );
    }

    #[test]
    fn parse_content_length_rejects_invalid_or_conflicting_values() {
        assert_eq!(parse_content_length(b"Content-Length: 12\r\n").expect("ok"), 12);
        assert_eq!(
            parse_content_length(b"Content-Length: 12\r\ncontent-length: 12\r\n").expect("ok"),
            12
        );
        assert!(parse_content_length(b"Content-Length: nope\r\n").is_err());
        assert!(parse_content_length(b"Content-Length: 12\r\nContent-Length: 13\r\n").is_err());
        assert!(parse_content_length(b"Host: localhost\r\n").is_err());
    }

    #[test]
    fn rpc_endpoint_rejects_conflicting_content_length_headers() {
        let daemon = RpcDaemon::test_instance();
        let request = b"POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 4\r\n\r\n";

        let err = handle_http_request_with_peer(
            &daemon,
            request,
            Some("127.0.0.1:1".parse().expect("socket")),
        )
        .expect_err("conflicting content-length should fail");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("conflicting content-length headers"));
    }

    #[test]
    fn request_parser_rejects_oversized_header_block_before_parsing() {
        let mut request = b"GET /healthz HTTP/1.1\r\n".to_vec();
        request.extend_from_slice(b"X-Oversized: ");
        request.extend(std::iter::repeat_n(b'a', MAX_HTTP_HEADER_LEN));
        request.extend_from_slice(b"\r\n\r\n");

        let err = request_method_path_headers(&request).expect_err("headers should be capped");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("headers too large"));
    }

    #[test]
    fn request_parser_rejects_oversized_header_line() {
        let mut request = b"GET /healthz HTTP/1.1\r\nX-Long: ".to_vec();
        request.extend(std::iter::repeat_n(b'a', MAX_HTTP_HEADER_LINE_LEN + 1));
        request.extend_from_slice(b"\r\n\r\n");

        let err = request_method_path_headers(&request).expect_err("header line should be capped");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("header line too large"));
    }

    #[test]
    fn request_parser_rejects_too_many_headers() {
        let mut request = b"GET /healthz HTTP/1.1\r\n".to_vec();
        for idx in 0..=MAX_HTTP_HEADER_COUNT {
            request.extend_from_slice(format!("X-Test-{idx}: yes\r\n").as_bytes());
        }
        request.extend_from_slice(b"\r\n");

        let err = request_method_path_headers(&request).expect_err("header count should be capped");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("too many headers"));
    }

    #[test]
    fn rpc_endpoint_rejects_oversized_content_length_without_overflow() {
        let daemon = RpcDaemon::test_instance();
        let request = format!(
            "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            codec::MAX_FRAME_PAYLOAD_LEN + 5
        );

        let err = handle_http_request_with_peer(
            &daemon,
            request.as_bytes(),
            Some("127.0.0.1:1".parse().expect("socket")),
        )
        .expect_err("oversized body should fail");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("body too large"));
    }
}
