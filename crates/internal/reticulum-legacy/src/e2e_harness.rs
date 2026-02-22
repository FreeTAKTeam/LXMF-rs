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
    let headers = &response[..header_end];
    let body_start = header_end + b"\r\n\r\n".len();
    let content_length = crate::rpc::http::parse_content_length(headers)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing content length"))?;
    if response.len() < body_start + content_length {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "response body incomplete"));
    }
    Ok(response[body_start..body_start + content_length].to_vec())
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
