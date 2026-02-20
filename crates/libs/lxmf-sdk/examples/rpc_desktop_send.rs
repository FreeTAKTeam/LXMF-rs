use lxmf_sdk::{Client, LxmfSdk, RpcBackendClient, SendRequest, StartRequest};
use serde::de::DeserializeOwned;
use serde_json::{json, Value as JsonValue};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("LXMF_RPC").unwrap_or_else(|_| "127.0.0.1:4242".to_owned());
    let source = std::env::var("LXMF_SOURCE").unwrap_or_else(|_| "example.desktop".to_owned());
    let destination =
        std::env::var("LXMF_DESTINATION").unwrap_or_else(|_| "example.peer".to_owned());

    let client = Client::new(RpcBackendClient::new(endpoint.clone()));
    let start_request: StartRequest = parse_struct(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": ["sdk.capability.cursor_replay"],
        "config": {
            "profile": "desktop-full",
            "bind_mode": "local_only",
            "auth_mode": "local_trusted",
            "overflow_policy": "reject",
            "block_timeout_ms": JsonValue::Null,
            "event_stream": {
                "max_poll_events": 128,
                "max_event_bytes": 32768,
                "max_batch_bytes": 1048576,
                "max_extension_keys": 32
            },
            "idempotency_ttl_ms": 86400000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false,
                "break_glass_ttl_ms": JsonValue::Null
            },
            "rpc_backend": {
                "listen_addr": endpoint,
                "read_timeout_ms": 5000,
                "write_timeout_ms": 5000,
                "max_header_bytes": 16384,
                "max_body_bytes": 1048576
            },
            "extensions": {}
        }
    }))?;
    let handle = client.start(start_request)?;
    println!(
        "started runtime_id={} contract_v{}",
        handle.runtime_id, handle.active_contract_version
    );

    let send_request: SendRequest = parse_struct(json!({
        "source": source,
        "destination": destination,
        "payload": {
            "title": "SDK Example",
            "content": "hello from lxmf-sdk example"
        },
        "idempotency_key": JsonValue::Null,
        "ttl_ms": 30000,
        "correlation_id": "example-rpc-desktop-send",
        "extensions": {}
    }))?;
    let message_id = client.send(send_request)?;
    println!("queued message_id={}", message_id.0);

    let batch = client.poll_events(None, 16)?;
    println!("polled events={} dropped={}", batch.events.len(), batch.dropped_count);

    Ok(())
}

fn parse_struct<T: DeserializeOwned>(value: JsonValue) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(value)?)
}
