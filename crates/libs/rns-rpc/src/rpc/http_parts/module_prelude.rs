use std::io;

use std::net::SocketAddr;

use crate::rpc::{codec, RpcDaemon, RpcRequest, RpcResponse};

use serde_json::json;

const HEADER_END: &[u8] = b"\r\n\r\n";

pub const MAX_HTTP_HEADER_LEN: usize = 64 * 1024;

pub const MAX_HTTP_HEADER_LINE_LEN: usize = 8 * 1024;

pub const MAX_HTTP_HEADER_COUNT: usize = 128;

pub type HttpHeaderList = Vec<(String, String)>;

pub type HttpRequestParts = (String, String, HttpHeaderList);

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
        let header_end = bounded_header_end(request)?;
        let headers = &request[..header_end];
        validate_header_block(headers)?;
        let parsed_headers = parse_headers(headers);
        let peer_ip = peer_addr.map(|addr| addr.ip().to_string());
        let body_start = header_end + HEADER_END.len();
        let (method, path) = parse_request_line(headers)?;
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
                let content_length = parse_content_length(headers)?;
                if content_length > codec::MAX_FRAME_PAYLOAD_LEN + 4 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
                }
                let body_end = body_start
                    .checked_add(content_length)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
                if request.len() < body_end {
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body incomplete"));
                }
                let body = &request[body_start..body_end];
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

fn bounded_header_end(request: &[u8]) -> io::Result<usize> {
    let search_len = request.len().min(MAX_HTTP_HEADER_LEN + HEADER_END.len());
    if let Some(header_end) = find_header_end(&request[..search_len]) {
        if header_end > MAX_HTTP_HEADER_LEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
        }
        return Ok(header_end);
    }
    if request.len() > MAX_HTTP_HEADER_LEN {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "missing headers"))
}

fn validate_header_block(headers: &[u8]) -> io::Result<()> {
    if headers.len() > MAX_HTTP_HEADER_LEN {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
    }
    let mut header_count = 0usize;
    for (idx, line) in headers.split(|byte| *byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.len() > MAX_HTTP_HEADER_LINE_LEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "header line too large"));
        }
        if idx > 0 && !line.is_empty() {
            header_count = header_count.saturating_add(1);
            if header_count > MAX_HTTP_HEADER_COUNT {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "too many headers"));
            }
        }
    }
    Ok(())
}

/// R: `Err` = no Content-Length header, a malformed value, or conflicting headers.
/// (No caller treats a missing header as a valid outcome, so absence is a failure.)
pub fn parse_content_length(headers: &[u8]) -> io::Result<usize> {
    let text = String::from_utf8_lossy(headers);
    let mut parsed = None;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            let value = rest.trim();
            let length = value.parse::<usize>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid content-length value")
            })?;
            if parsed.is_some_and(|existing| existing != length) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "conflicting content-length headers",
                ));
            }
            parsed = Some(length);
        }
    }
    parsed.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing content-length"))
}

pub fn request_method_path_headers(request: &[u8]) -> io::Result<HttpRequestParts> {
    let header_end = bounded_header_end(request)?;
    let headers = &request[..header_end];
    validate_header_block(headers)?;
    let parsed_headers = parse_headers(headers);
    let (method, path) = parse_request_line(headers)?;
    let (path_only, _) = split_path_and_query(path.as_str());
    Ok((method, path_only.to_owned(), parsed_headers))
}

pub fn streaming_event_response_header() -> Vec<u8> {
    let mut response = Vec::new();
    response.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
    response.extend_from_slice(b"Content-Type: application/msgpack\r\n");
    response.extend_from_slice(b"Cache-Control: no-store\r\n");
    response.extend_from_slice(b"Connection: close\r\n");
    response.extend_from_slice(b"\r\n");
    response
}

/// R: `Err` = no request line or missing method/path (always an invalid request).
fn parse_request_line(headers: &[u8]) -> io::Result<(String, String)> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid request line");
    let text = String::from_utf8_lossy(headers);
    let mut lines = text.lines();
    let line = lines.next().ok_or_else(invalid)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or_else(invalid)?.to_string();
    let path = parts.next().ok_or_else(invalid)?.to_string();
    Ok((method, path))
}

fn parse_headers(headers: &[u8]) -> HttpHeaderList {
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
            return Some(percent_decode(value).unwrap_or_else(|err| {
                log::debug!("invalid percent-encoding in query param {key}: {err}");
                value.to_string()
            }));
        }
    }
    None
}

/// R: `Err` = a complete `%XY` escape with invalid hex digits, or non-UTF-8 result.
/// (A truncated trailing `%`/`%X` is kept literally, matching prior behaviour.)
fn percent_decode(value: &str) -> io::Result<String> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid percent-encoding");
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
                let hi = decode_hex(bytes[idx + 1]).ok_or_else(invalid)?;
                let lo = decode_hex(bytes[idx + 2]).ok_or_else(invalid)?;
                out.push((hi << 4) | lo);
                idx += 3;
            }
            byte => {
                out.push(byte);
                idx += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
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

pub fn build_rpc_error_response(id: u64, error: crate::rpc::RpcError) -> io::Result<Vec<u8>> {
    let response = RpcResponse { id, result: None, error: Some(error) };
    let body = codec::encode_frame(&response).map_err(io::Error::other)?;
    Ok(build_response(StatusCode::Ok, &body))
}

pub fn build_error_response(message: &str) -> Vec<u8> {
    let body = message.as_bytes();
    build_response(StatusCode::BadRequest, body)
}
