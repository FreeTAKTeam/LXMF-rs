use reticulum_daemon::text::decode_utf8;
use rns_rpc::rpc::codec;
use rns_rpc::{http, RpcRequest, RpcResponse};
use serde_json::json;
use std::io::IsTerminal;
use std::net::SocketAddr;

#[derive(Debug, Default, Clone)]
pub(super) struct RpcRequestLogMeta {
    http_method: String,
    path: String,
    rpc_method: Option<String>,
    rpc_request_id: Option<u64>,
    trace_ref: Option<String>,
}

pub(super) fn parse_request_log_meta(request: &[u8]) -> RpcRequestLogMeta {
    let mut meta = RpcRequestLogMeta::default();
    let Some(header_end) = http::find_header_end(request) else {
        return meta;
    };
    let headers = &request[..header_end];
    let Ok((http_method, path)) = parse_http_request_line(headers) else {
        return meta;
    };
    meta.http_method = http_method.to_string();
    meta.path = path.to_string();

    if http_method != "POST" || path != "/rpc" {
        return meta;
    }
    let Ok(content_length) = http::parse_content_length(headers) else {
        return meta;
    };
    if content_length > codec::MAX_FRAME_PAYLOAD_LEN + 4 {
        return meta;
    }
    let Some(body_start) = header_end.checked_add(4) else {
        return meta;
    };
    let Some(body_end) = body_start.checked_add(content_length) else {
        return meta;
    };
    if request.len() < body_end {
        return meta;
    }
    let body = &request[body_start..body_end];
    let Ok(rpc_request) = codec::decode_frame::<RpcRequest>(body) else {
        return meta;
    };
    meta.trace_ref = Some(format!("rpc:{}:{:016x}", rpc_request.method, rpc_request.id));
    meta.rpc_method = Some(rpc_request.method);
    meta.rpc_request_id = Some(rpc_request.id);
    meta
}

pub(super) fn emit_rpc_access_log(
    peer_addr: SocketAddr,
    meta: &RpcRequestLogMeta,
    response: &[u8],
    elapsed_ms: u64,
    error_text: Option<&str>,
) {
    let status_code = parse_status_code(response).unwrap_or(0);
    let rpc_error = parse_rpc_response_error(response).ok().flatten();
    let effective_error = error_text
        .map(str::to_string)
        .or_else(|| rpc_error.as_ref().map(|(_, message)| message.clone()));
    if pretty_console_logs_enabled() {
        log::info!(
            "{} {} {} {} {}{}{}",
            pretty_tag("rpc", 34),
            pretty_status(&status_code.to_string(), status_code_color(status_code)),
            pretty_elapsed(elapsed_ms),
            pretty_method(&format!("{} {}", meta.http_method, meta.path)),
            pretty_secondary(&format!("peer={peer_addr}")),
            meta.rpc_method
                .as_ref()
                .map(|method| format!(" {}", pretty_secondary(&format!("rpc={method}"))))
                .unwrap_or_default(),
            effective_error
                .as_deref()
                .map(|error| format!(" {}", pretty_error(error)))
                .unwrap_or_default()
        );
        return;
    }
    let (rpc_error_code, rpc_error_message) =
        rpc_error.map_or((None, None), |(code, message)| (Some(code), Some(message)));
    let payload = json!({
        "event": "rpc_request",
        "peer": peer_addr.to_string(),
        "http_method": meta.http_method,
        "path": meta.path,
        "rpc_method": meta.rpc_method,
        "rpc_request_id": meta.rpc_request_id,
        "trace_ref": meta.trace_ref,
        "status_code": status_code,
        "rpc_error_code": rpc_error_code,
        "rpc_error_message": rpc_error_message,
        "elapsed_ms": elapsed_ms,
        "ok": (200..=299).contains(&status_code) && effective_error.is_none(),
        "error": effective_error,
    });
    log::info!("{}", payload);
}

fn parse_http_request_line(headers: &[u8]) -> Result<(&str, &str), &'static str> {
    let text = decode_utf8(headers, "RPC request headers")
        .map_err(|_| "invalid UTF-8 in request headers")?;
    let line = text.lines().next().ok_or("no HTTP request line")?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("no HTTP method")?;
    let path = parts.next().ok_or("no HTTP path")?;
    Ok((method, path))
}

fn parse_status_code(response: &[u8]) -> Result<u16, &'static str> {
    let line_end = response
        .windows(2)
        .position(|window| window == b"\r\n")
        .ok_or("no response status line")?;
    let text = decode_utf8(&response[..line_end], "RPC response status")
        .map_err(|_| "invalid UTF-8 in response")?;
    let line = text.lines().next().ok_or("no response status line")?;
    let mut parts = line.split_whitespace();
    let _http_version = parts.next().ok_or("no HTTP version")?;
    let code = parts.next().ok_or("no status code")?;
    code.parse::<u16>().map_err(|_| "invalid status code")
}

fn parse_rpc_response_error(response: &[u8]) -> Result<Option<(String, String)>, &'static str> {
    let header_end = http::find_header_end(response).ok_or("no header end in response")?;
    let headers = &response[..header_end];
    let content_length =
        http::parse_content_length(headers).map_err(|_| "invalid content-length in response")?;
    if content_length > codec::MAX_FRAME_PAYLOAD_LEN + 4 {
        return Err("response body exceeds frame limit");
    }
    let body_start = header_end.checked_add(4).ok_or("body start overflow")?;
    let body_end = body_start.checked_add(content_length).ok_or("body end overflow")?;
    if response.len() < body_end {
        return Err("response body truncated");
    }
    let rpc_response = codec::decode_frame::<RpcResponse>(&response[body_start..body_end])
        .map_err(|_| "failed to decode RPC response body")?;
    Ok(rpc_response.error.map(|e| (e.code, e.message)))
}

fn pretty_console_logs_enabled() -> bool {
    matches!(
        std::env::var("LXMF_LOG_PRETTY").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

fn pretty_color_enabled() -> bool {
    if matches!(
        std::env::var("LXMF_LOG_COLOR").ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" | "always" | "ALWAYS")
    ) {
        return true;
    }
    if matches!(
        std::env::var("LXMF_LOG_COLOR").ok().as_deref(),
        Some("0" | "false" | "FALSE" | "no" | "NO" | "off" | "OFF" | "never" | "NEVER")
    ) {
        return false;
    }
    pretty_console_logs_enabled() && std::io::stderr().is_terminal()
}

fn ansi(text: &str, code: &str) -> String {
    if pretty_color_enabled() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn pretty_tag(label: &str, color: u8) -> String {
    ansi(&format!("[{label:<4}]"), &color.to_string())
}

fn pretty_status(label: &str, color: u8) -> String {
    ansi(&format!("{label:<4}"), &format!("1;{color}"))
}

fn pretty_elapsed(elapsed_ms: u64) -> String {
    ansi(&format!("{elapsed_ms:>4}ms"), "2")
}

fn pretty_method(method: &str) -> String {
    ansi(method, "1")
}

fn pretty_secondary(value: &str) -> String {
    ansi(value, "2")
}

fn pretty_error(value: &str) -> String {
    ansi(&format!("error={value}"), "31")
}

fn status_code_color(status_code: u16) -> u8 {
    match status_code {
        200..=299 => 32,
        400..=499 => 33,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_log_meta_extracts_rpc_fields() {
        let rpc_body = codec::encode_frame(&RpcRequest {
            id: 44,
            method: "sdk_poll_events_v2".to_string(),
            params: Some(json!({ "cursor": null, "max": 1 })),
        })
        .expect("encode rpc body");
        let request = format!(
            "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            rpc_body.len()
        );
        let mut raw = request.into_bytes();
        raw.extend_from_slice(&rpc_body);

        let meta = parse_request_log_meta(&raw);
        assert_eq!(meta.http_method, "POST");
        assert_eq!(meta.path, "/rpc");
        assert_eq!(meta.rpc_method.as_deref(), Some("sdk_poll_events_v2"));
        assert_eq!(meta.rpc_request_id, Some(44));
        assert!(meta
            .trace_ref
            .as_deref()
            .is_some_and(|value| value.contains("sdk_poll_events_v2")));
    }

    #[test]
    fn parse_request_log_meta_keeps_non_rpc_requests_lightweight() {
        let raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let meta = parse_request_log_meta(raw);
        assert_eq!(meta.http_method, "GET");
        assert_eq!(meta.path, "/healthz");
        assert!(meta.rpc_method.is_none());
        assert!(meta.rpc_request_id.is_none());
        assert!(meta.trace_ref.is_none());
    }

    #[test]
    fn parse_request_log_meta_ignores_oversized_rpc_body_length() {
        let request = format!(
            "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            codec::MAX_FRAME_PAYLOAD_LEN + 5
        );

        let meta = parse_request_log_meta(request.as_bytes());

        assert_eq!(meta.http_method, "POST");
        assert_eq!(meta.path, "/rpc");
        assert!(meta.rpc_method.is_none());
        assert!(meta.rpc_request_id.is_none());
        assert!(meta.trace_ref.is_none());
    }

    #[test]
    fn parse_request_log_meta_ignores_incomplete_rpc_body() {
        let raw = b"POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: 8\r\n\r\nshort";

        let meta = parse_request_log_meta(raw);

        assert_eq!(meta.http_method, "POST");
        assert_eq!(meta.path, "/rpc");
        assert!(meta.rpc_method.is_none());
        assert!(meta.rpc_request_id.is_none());
        assert!(meta.trace_ref.is_none());
    }

    #[test]
    fn parse_status_code_extracts_numeric_status() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(parse_status_code(response), Ok(200));
    }

    #[test]
    fn parse_status_code_ignores_binary_rpc_body() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n\xff\x00\x91";
        assert_eq!(parse_status_code(response), Ok(200));
    }

    #[test]
    fn parse_rpc_response_error_extracts_json_rpc_error_from_http_ok() {
        let rpc_body = codec::encode_frame(&RpcResponse {
            id: 44,
            result: None,
            error: Some(rns_rpc::RpcError::new("SDK_INTERNAL", "boom")),
        })
        .expect("encode rpc body");
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", rpc_body.len());
        let mut raw = response.into_bytes();
        raw.extend_from_slice(&rpc_body);

        assert_eq!(
            parse_rpc_response_error(&raw),
            Ok(Some(("SDK_INTERNAL".to_string(), "boom".to_string())))
        );
    }
}
