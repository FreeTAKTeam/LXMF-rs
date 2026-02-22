use std::io;
use std::net::SocketAddr;

use crate::rpc::{codec, RpcDaemon, RpcRequest, RpcResponse};
use serde_json::json;

const HEADER_END: &[u8] = b"\r\n\r\n";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransportAuthContext {
    pub client_cert_present: bool,
    pub client_subject: Option<String>,
    pub client_sans: Vec<String>,
}

pub fn handle_http_request(daemon: &RpcDaemon, request: &[u8]) -> io::Result<Vec<u8>> {
    let _ = daemon;
    let _ = request;
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "peer address is required; use handle_http_request_with_peer",
    ))
}

pub fn handle_http_request_with_peer(
    daemon: &RpcDaemon,
    request: &[u8],
    peer_addr: Option<SocketAddr>,
) -> io::Result<Vec<u8>> {
    handle_http_request_with_transport_auth(daemon, request, peer_addr, None)
}

pub fn handle_http_request_with_transport_auth(
    daemon: &RpcDaemon,
    request: &[u8],
    peer_addr: Option<SocketAddr>,
    transport_auth: Option<TransportAuthContext>,
) -> io::Result<Vec<u8>> {
    let response = (|| -> io::Result<Vec<u8>> {
        let header_end = find_header_end(request)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing headers"))?;
        let headers = &request[..header_end];
        let parsed_headers = parse_headers(headers);
        let peer_ip = peer_addr.map(|addr| addr.ip().to_string());
        let body_start = header_end + HEADER_END.len();
        let (method, path) = parse_request_line(headers)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid request line"))?;
        let (path_only, query) = split_path_and_query(path.as_str());
        daemon.metrics_record_http_request(method.as_str(), path_only);
        match (method.as_str(), path_only) {
            ("GET", "/healthz") => {
                let body = serde_json::to_vec(&json!({
                    "ok": true,
                    "service": "reticulumd-rpc",
                    "status": "healthy",
                }))
                .map_err(io::Error::other)?;
                Ok(build_json_response(StatusCode::Ok, &body))
            }
            ("GET", "/readyz") => {
                let body = serde_json::to_vec(&json!({
                    "ok": true,
                    "service": "reticulumd-rpc",
                    "status": "ready",
                }))
                .map_err(io::Error::other)?;
                Ok(build_json_response(StatusCode::Ok, &body))
            }
            ("GET", "/livez") => {
                let body = serde_json::to_vec(&json!({
                    "ok": true,
                    "service": "reticulumd-rpc",
                    "status": "alive",
                }))
                .map_err(io::Error::other)?;
                Ok(build_json_response(StatusCode::Ok, &body))
            }
            ("GET", "/metrics") => {
                if let Err(error) = daemon.authorize_http_request_with_transport(
                    &parsed_headers,
                    peer_ip.as_deref(),
                    transport_auth.as_ref(),
                ) {
                    return build_rpc_error_response(0, error);
                }
                let body =
                    serde_json::to_vec(&daemon.metrics_snapshot()).map_err(io::Error::other)?;
                Ok(build_json_response(StatusCode::Ok, &body))
            }
            ("GET", "/events") if query.is_empty() => {
                if let Err(error) = daemon.authorize_http_request_with_transport(
                    &parsed_headers,
                    peer_ip.as_deref(),
                    transport_auth.as_ref(),
                ) {
                    return build_rpc_error_response(0, error);
                }
                if let Some(event) = daemon.take_event() {
                    let body = codec::encode_frame(&event).map_err(io::Error::other)?;
                    Ok(build_response(StatusCode::Ok, &body))
                } else {
                    Ok(build_response(StatusCode::NoContent, &[]))
                }
            }
            ("GET", "/events") | ("GET", "/events/v2") => {
                if let Err(error) = daemon.authorize_http_request_with_transport(
                    &parsed_headers,
                    peer_ip.as_deref(),
                    transport_auth.as_ref(),
                ) {
                    return build_rpc_error_response(0, error);
                }
                let cursor = query_param(query, "cursor");
                let max = match query_param(query, "max") {
                    Some(raw) => raw.parse::<usize>().unwrap_or(0),
                    None => 64,
                };
                let response = daemon.handle_rpc(RpcRequest {
                    id: 0,
                    method: "sdk_poll_events_v2".to_string(),
                    params: Some(json!({
                        "cursor": cursor,
                        "max": max,
                    })),
                })?;
                let body = codec::encode_frame(&response).map_err(io::Error::other)?;
                Ok(build_response(StatusCode::Ok, &body))
            }
            ("POST", "/rpc") => {
                let content_length = parse_content_length(headers).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "missing content-length")
                })?;
                if request.len() < body_start + content_length {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body incomplete"));
                }
                let body = &request[body_start..body_start + content_length];
                let rpc_request: RpcRequest = codec::decode_frame(body)?;
                if let Err(error) = daemon.authorize_http_request_with_transport(
                    &parsed_headers,
                    peer_ip.as_deref(),
                    transport_auth.as_ref(),
                ) {
                    return build_rpc_error_response(rpc_request.id, error);
                }
                let rpc_response = daemon.handle_rpc(rpc_request)?;
                let response_body = codec::encode_frame(&rpc_response).map_err(io::Error::other)?;
                Ok(build_response(StatusCode::Ok, &response_body))
            }
            _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported request")),
        }
    })();
    if response.is_err() {
        daemon.metrics_record_http_error();
    }
    response
}

pub fn find_header_end(request: &[u8]) -> Option<usize> {
    request.windows(HEADER_END.len()).position(|window| window == HEADER_END)
}

pub fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(headers);
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            let value = rest.trim();
            if let Ok(length) = value.parse::<usize>() {
                return Some(length);
            }
        }
    }
    None
}

fn parse_request_line(headers: &[u8]) -> Option<(String, String)> {
    let text = String::from_utf8_lossy(headers);
    let mut lines = text.lines();
    let line = lines.next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    Some((method, path))
}

fn parse_headers(headers: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(headers)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn split_path_and_query(path: &str) -> (&str, &str) {
    match path.split_once('?') {
        Some((path_only, query)) => (path_only, query),
        None => (path, ""),
    }
}

fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == key {
            return percent_decode(value).or_else(|| Some(value.to_string()));
        }
    }
    None
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'+' => {
                out.push(b' ');
                idx += 1;
            }
            b'%' if idx + 2 < bytes.len() => {
                let hi = decode_hex(bytes[idx + 1])?;
                let lo = decode_hex(bytes[idx + 2])?;
                out.push((hi << 4) | lo);
                idx += 3;
            }
            byte => {
                out.push(byte);
                idx += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn decode_hex(input: u8) -> Option<u8> {
    match input {
        b'0'..=b'9' => Some(input - b'0'),
        b'a'..=b'f' => Some(input - b'a' + 10),
        b'A'..=b'F' => Some(input - b'A' + 10),
        _ => None,
    }
}

enum StatusCode {
    Ok,
    NoContent,
    BadRequest,
}

fn build_response(status: StatusCode, body: &[u8]) -> Vec<u8> {
    build_response_with_content_type(status, body, "application/msgpack")
}

fn build_json_response(status: StatusCode, body: &[u8]) -> Vec<u8> {
    build_response_with_content_type(status, body, "application/json")
}

fn build_response_with_content_type(
    status: StatusCode,
    body: &[u8],
    content_type: &str,
) -> Vec<u8> {
    let status_line = match status {
        StatusCode::Ok => "HTTP/1.1 200 OK",
        StatusCode::NoContent => "HTTP/1.1 204 No Content",
        StatusCode::BadRequest => "HTTP/1.1 400 Bad Request",
    };
    let mut response = Vec::new();
    response.extend_from_slice(status_line.as_bytes());
    response.extend_from_slice(format!("\r\nContent-Type: {content_type}\r\n").as_bytes());
    response.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    response.extend_from_slice(b"\r\n");
    response.extend_from_slice(body);
    response
}

fn build_rpc_error_response(id: u64, error: crate::rpc::RpcError) -> io::Result<Vec<u8>> {
    let response = RpcResponse { id, result: None, error: Some(error) };
    let body = codec::encode_frame(&response).map_err(io::Error::other)?;
    Ok(build_response(StatusCode::Ok, &body))
}

pub fn build_error_response(message: &str) -> Vec<u8> {
    let body = message.as_bytes();
    build_response(StatusCode::BadRequest, body)
}

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
}
