use lxmf_sdk::{Client, LxmfSdk, LxmfSdkManualTick, RpcBackendClient, StartRequest, TickBudget};
use serde::de::DeserializeOwned;
use serde_json::{json, Value as JsonValue};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("LXMF_RPC").unwrap_or_else(|_| "127.0.0.1:4242".to_owned());
    let client = Client::new(RpcBackendClient::new(endpoint.clone()));

    let start_request: StartRequest = parse_struct(json!({
        "supported_contract_versions": [2],
        "requested_capabilities": [],
        "config": {
            "profile": "embedded-alloc",
            "bind_mode": "local_only",
            "auth_mode": "local_trusted",
            "overflow_policy": "reject",
            "block_timeout_ms": JsonValue::Null,
            "event_stream": {
                "max_poll_events": 64,
                "max_event_bytes": 8192,
                "max_batch_bytes": 65536,
                "max_extension_keys": 8
            },
            "idempotency_ttl_ms": 60000,
            "redaction": {
                "enabled": true,
                "sensitive_transform": "hash",
                "break_glass_allowed": false,
                "break_glass_ttl_ms": JsonValue::Null
            },
            "rpc_backend": {
                "listen_addr": endpoint,
                "read_timeout_ms": 2000,
                "write_timeout_ms": 2000,
                "max_header_bytes": 8192,
                "max_body_bytes": 65536
            },
            "extensions": {}
        }
    }))?;

    let handle = client.start(start_request)?;
    println!("embedded profile started: runtime_id={}", handle.runtime_id);

    let budget: TickBudget = parse_struct(json!({
        "max_work_items": 64,
        "max_duration_ms": 25
    }))?;
    let tick_result = client.tick(budget)?;
    println!(
        "tick processed_items={} yielded={} next_delay_ms={:?}",
        tick_result.processed_items, tick_result.yielded, tick_result.next_recommended_delay_ms
    );

    Ok(())
}

fn parse_struct<T: DeserializeOwned>(value: JsonValue) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(value)?)
}
