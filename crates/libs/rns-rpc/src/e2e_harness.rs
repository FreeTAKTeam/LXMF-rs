use std::io;

pub fn is_ready_line(line: &str) -> bool {
    line.contains("listening on http://")
}

pub fn build_rpc_body(
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<String, serde_json::Error> {
    let request = crate::rpc::RpcRequest { id, method: method.to_string(), params };
    serde_json::to_string(&request)
}

pub fn parse_rpc_response(input: &str) -> Result<crate::rpc::RpcResponse, serde_json::Error> {
    serde_json::from_str(input)
}

pub fn build_rpc_frame(
    id: u64,
    method: &str,
    params: Option<serde_json::Value>,
) -> io::Result<Vec<u8>> {
    let request = crate::rpc::RpcRequest { id, method: method.to_string(), params };
    crate::rpc::codec::encode_frame(&request)
}

pub fn parse_rpc_frame(bytes: &[u8]) -> io::Result<crate::rpc::RpcResponse> {
    crate::rpc::codec::decode_frame(bytes)
}

pub fn build_http_post(path: &str, host: &str, body: &[u8]) -> Vec<u8> {
    let mut request = Vec::new();
    request.extend_from_slice(format!("POST {} HTTP/1.1\r\n", path).as_bytes());
    request.extend_from_slice(format!("Host: {}\r\n", host).as_bytes());
    request.extend_from_slice(b"Content-Type: application/msgpack\r\n");
    request.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    request.extend_from_slice(b"\r\n");
    request.extend_from_slice(body);
    request
}

pub fn parse_http_response_body(response: &[u8]) -> io::Result<Vec<u8>> {
    let header_end = crate::rpc::http::find_header_end(response)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing headers"))?;
    if header_end > crate::rpc::http::MAX_HTTP_HEADER_LEN {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
    }
    let headers = &response[..header_end];
    let body_start = header_end
        .checked_add(b"\r\n\r\n".len())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "response too large"))?;
    let content_length = crate::rpc::http::parse_content_length(headers)?;
    if content_length > crate::rpc::codec::MAX_FRAME_PAYLOAD_LEN + 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let body_end = body_start
        .checked_add(content_length)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "response too large"))?;
    if response.len() < body_end {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "response body incomplete"));
    }
    Ok(response[body_start..body_end].to_vec())
}

pub fn build_daemon_args(
    rpc: &str,
    db_path: &str,
    announce_interval_secs: u64,
    transport: Option<&str>,
    config: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "--rpc".to_string(),
        rpc.to_string(),
        "--db".to_string(),
        db_path.to_string(),
        "--announce-interval-secs".to_string(),
        announce_interval_secs.to_string(),
    ];

    if let Some(transport) = transport {
        args.push("--transport".to_string());
        args.push(transport.to_string());
    }

    if let Some(config) = config {
        args.push("--config".to_string());
        args.push(config.to_string());
    }

    args
}

pub fn build_send_params(
    message_id: &str,
    source: &str,
    destination: &str,
    content: &str,
) -> serde_json::Value {
    serde_json::json!({
        "id": message_id,
        "source": source,
        "destination": destination,
        "content": content,
        "fields": serde_json::Value::Null,
    })
}

pub fn build_tcp_client_config(host: &str, port: u16) -> String {
    format!(
        "[[interfaces]]\ntype = \"tcp_client\"\nenabled = true\nhost = \"{}\"\nport = {}\n",
        host, port
    )
}

pub fn message_present(response: &crate::rpc::RpcResponse, message_id: &str) -> bool {
    let Some(result) = response.result.as_ref() else {
        return false;
    };
    let Some(messages) = result.get("messages").and_then(|value| value.as_array()) else {
        return false;
    };
    messages
        .iter()
        .any(|message| message.get("id").and_then(|value| value.as_str()) == Some(message_id))
}

pub fn timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

pub fn peer_present(response: &crate::rpc::RpcResponse, peer: &str) -> bool {
    let Some(result) = response.result.as_ref() else {
        return false;
    };
    let Some(peers) = result.get("peers").and_then(|value| value.as_array()) else {
        return false;
    };
    peers.iter().any(|entry| entry.get("peer").and_then(|value| value.as_str()) == Some(peer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_response_body_rejects_oversized_header_block() {
        let mut response = b"HTTP/1.1 200 OK\r\n".to_vec();
        response.extend(std::iter::repeat_n(b'a', crate::rpc::http::MAX_HTTP_HEADER_LEN));
        response.extend_from_slice(b"\r\nContent-Length: 0\r\n\r\n");

        let err = parse_http_response_body(&response).expect_err("headers should be capped");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("headers too large"));
    }

    #[test]
    fn parse_http_response_body_rejects_oversized_declared_body_before_waiting() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            crate::rpc::codec::MAX_FRAME_PAYLOAD_LEN + 5
        );

        let err = parse_http_response_body(response.as_bytes())
            .expect_err("oversized declared body should fail");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("body too large"));
    }

    #[test]
    fn parse_http_response_body_reports_incomplete_bounded_body() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nshort";

        let err = parse_http_response_body(response).expect_err("short body should fail");

        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        assert!(err.to_string().contains("response body incomplete"));
    }

    #[test]
    fn parse_http_response_body_rejects_conflicting_content_lengths() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 4\r\n\r\nbody";

        let err = parse_http_response_body(response)
            .expect_err("conflicting content-length headers should fail");

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("conflicting content-length headers"));
    }
}
