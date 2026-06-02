use serde_json::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

pub(crate) fn spawn_on_inbound_watcher(
    rpc_addr: String,
    command: String,
    messages_dir: Option<PathBuf>,
) {
    thread::spawn(move || {
        let mut cursor: Option<String> = None;
        loop {
            match poll_event_batch(&rpc_addr, cursor.as_deref()) {
                Ok((events, next_cursor)) => {
                    cursor = next_cursor;
                    if events.is_empty() {
                        thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                    for event in events {
                        if let Err(err) = crate::inbound::run_on_inbound_command(
                            &command,
                            &event,
                            messages_dir.as_deref(),
                        ) {
                            log::error!("on-inbound hook failed: {err}");
                        }
                    }
                }
                Err(err)
                    if err.contains("SDK_RUNTIME_CURSOR_EXPIRED")
                        || err.contains("SDK_RUNTIME_STREAM_DEGRADED") =>
                {
                    cursor = None;
                    thread::sleep(Duration::from_millis(250));
                }
                Err(err) => {
                    log::error!("inbound event watcher stopped: {err}");
                    break;
                }
            }
        }
    });
}

fn poll_event_batch(
    rpc_addr: &str,
    cursor: Option<&str>,
) -> Result<(Vec<serde_json::Value>, Option<String>), String> {
    let response = rpc_call(
        rpc_addr,
        "sdk_poll_events_v2",
        Some(json!({
            "cursor": cursor,
            "max": 256,
        })),
    )?;
    let result = response.get("result").unwrap_or(&response);
    if let Some(error) = response.get("error").or_else(|| result.get("error")) {
        let code = error.get("code").and_then(|value| value.as_str()).unwrap_or("RPC_ERROR");
        let message =
            error.get("message").and_then(|value| value.as_str()).unwrap_or("unknown rpc error");
        return Err(format!("{code}: {message}"));
    }
    let events =
        result.get("events").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let next_cursor =
        result.get("next_cursor").and_then(|value| value.as_str()).map(ToOwned::to_owned);
    Ok((events, next_cursor))
}

pub(crate) fn rpc_call(
    rpc_addr: &str,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let payload = encode_rpc_frame(json!({
        "id": 1u64,
        "method": method,
        "params": params,
    }))?;
    let request = format!(
        "POST /rpc HTTP/1.1\r\nHost: {rpc_addr}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let mut request_bytes = request.into_bytes();
    request_bytes.extend_from_slice(&payload);
    let response = http_request_bytes(rpc_addr, &request_bytes)?;
    let body = http_body(&response).ok_or_else(|| "rpc response missing body".to_string())?;
    if let Some(status) = http_status_code(&response) {
        if status >= 400 && !looks_like_rpc_frame(body) {
            let message = String::from_utf8_lossy(body).trim().to_string();
            return Err(if message.is_empty() {
                format!("rpc http error {status}")
            } else {
                message
            });
        }
    }
    decode_rpc_frame(body)
}

pub(crate) fn http_request_bytes(rpc_addr: &str, request: &[u8]) -> Result<Vec<u8>, String> {
    let addr = resolve_socket_addr(rpc_addr)?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|err| err.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(|err| err.to_string())?;
    stream.set_write_timeout(Some(Duration::from_secs(2))).map_err(|err| err.to_string())?;
    stream.write_all(request).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).map_err(|err| err.to_string())?;
    Ok(bytes)
}

fn resolve_socket_addr(rpc_addr: &str) -> Result<SocketAddr, String> {
    rpc_addr
        .to_socket_addrs()
        .map_err(|err| err.to_string())?
        .next()
        .ok_or_else(|| format!("failed to resolve rpc address {rpc_addr}"))
}

fn http_body(response: &[u8]) -> Option<&[u8]> {
    response.windows(4).position(|window| window == b"\r\n\r\n").map(|index| &response[index + 4..])
}

fn http_status_code(response: &[u8]) -> Option<u16> {
    let header_end = response.windows(2).position(|window| window == b"\r\n")?;
    let status_line = std::str::from_utf8(&response[..header_end]).ok()?;
    let mut parts = status_line.split_whitespace();
    let _http = parts.next()?;
    parts.next()?.parse::<u16>().ok()
}

fn looks_like_rpc_frame(body: &[u8]) -> bool {
    if body.len() < 4 {
        return false;
    }
    let len = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    body.len() >= len + 4
}

pub(crate) fn encode_rpc_frame(value: serde_json::Value) -> Result<Vec<u8>, String> {
    let payload = rmp_serde::to_vec(&value).map_err(|err| err.to_string())?;
    let len = u32::try_from(payload.len()).map_err(|_| "rpc frame too large".to_string())?;
    let mut framed = Vec::with_capacity(payload.len() + 4);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

pub(crate) fn decode_rpc_frame(bytes: &[u8]) -> Result<serde_json::Value, String> {
    if bytes.len() < 4 {
        return Err("rpc response too short".to_string());
    }
    let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < len + 4 {
        return Err("rpc response incomplete".to_string());
    }
    let value: serde_json::Value =
        rmp_serde::from_slice(&bytes[4..4 + len]).map_err(|err| err.to_string())?;
    Ok(normalize_rpc_response(value))
}

fn normalize_rpc_response(value: serde_json::Value) -> serde_json::Value {
    let Some(items) = value.as_array() else {
        return value;
    };
    if items.len() != 3 {
        return value;
    }

    let id = items.first().cloned().unwrap_or(serde_json::Value::Null);
    let result = items.get(1).cloned().unwrap_or(serde_json::Value::Null);
    let error = items.get(2).cloned().unwrap_or(serde_json::Value::Null);
    let mut map = serde_json::Map::new();
    map.insert("id".to_string(), id);
    if !result.is_null() {
        map.insert("result".to_string(), result);
    }
    let error = normalize_rpc_error(error);
    if !error.is_null() {
        map.insert("error".to_string(), error);
    }
    serde_json::Value::Object(map)
}

fn normalize_rpc_error(value: serde_json::Value) -> serde_json::Value {
    let Some(items) = value.as_array() else {
        return value;
    };
    if items.is_empty() {
        return serde_json::Value::Null;
    }

    json!({
        "code": items.first().and_then(|entry| entry.as_str()).unwrap_or_default(),
        "message": items.get(1).and_then(|entry| entry.as_str()).unwrap_or_default(),
        "machine_code": items.get(2).cloned().unwrap_or(serde_json::Value::Null),
        "category": items.get(3).cloned().unwrap_or(serde_json::Value::Null),
        "retryable": items.get(4).cloned().unwrap_or(serde_json::Value::Null),
        "is_user_actionable": items.get(5).cloned().unwrap_or(serde_json::Value::Null),
    })
}
