use std::io;

use crate::rpc::{codec, handle_framed_request, RpcDaemon};

const HEADER_END: &[u8] = b"\r\n\r\n";

pub fn handle_http_request(daemon: &RpcDaemon, request: &[u8]) -> io::Result<Vec<u8>> {
    let header_end = find_header_end(request)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing headers"))?;
    let headers = &request[..header_end];
    let body_start = header_end + HEADER_END.len();
    let (method, path) = parse_request_line(headers)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid request line"))?;
    match (method.as_str(), path.as_str()) {
        ("GET", "/events") => {
            if let Some(event) = daemon.take_event() {
                let body = codec::encode_frame(&event).map_err(io::Error::other)?;
                Ok(build_response(StatusCode::Ok, &body))
            } else {
                Ok(build_response(StatusCode::NoContent, &[]))
            }
        }
        ("POST", "/rpc") => {
            let content_length = parse_content_length(headers).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "missing content-length")
            })?;
            if request.len() < body_start + content_length {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body incomplete"));
            }
            let body = &request[body_start..body_start + content_length];
            let response_body = handle_framed_request(daemon, body)?;
            Ok(build_response(StatusCode::Ok, &response_body))
        }
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported request")),
    }
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

enum StatusCode {
    Ok,
    NoContent,
    BadRequest,
}

fn build_response(status: StatusCode, body: &[u8]) -> Vec<u8> {
    let status_line = match status {
        StatusCode::Ok => "HTTP/1.1 200 OK",
        StatusCode::NoContent => "HTTP/1.1 204 No Content",
        StatusCode::BadRequest => "HTTP/1.1 400 Bad Request",
    };
    let mut response = Vec::new();
    response.extend_from_slice(status_line.as_bytes());
    response.extend_from_slice(b"\r\nContent-Type: application/msgpack\r\n");
    response.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    response.extend_from_slice(b"\r\n");
    response.extend_from_slice(body);
    response
}

pub fn build_error_response(message: &str) -> Vec<u8> {
    let body = message.as_bytes();
    build_response(StatusCode::BadRequest, body)
}
