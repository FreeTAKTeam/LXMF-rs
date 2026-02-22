use lxmf_sdk::{Client, LxmfSdk, RpcBackendClient, SdkConfig, SendRequest, StartRequest};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::var("LXMF_RPC").unwrap_or_else(|_| "127.0.0.1:4242".to_owned());
    let source = std::env::var("LXMF_SOURCE").unwrap_or_else(|_| "example.desktop".to_owned());
    let destination =
        std::env::var("LXMF_DESTINATION").unwrap_or_else(|_| "example.peer".to_owned());

    let client = Client::new(RpcBackendClient::new(endpoint.clone()));
    let start_request =
        StartRequest::new(SdkConfig::desktop_full_default().with_rpc_listen_addr(endpoint))
            .with_requested_capability("sdk.capability.cursor_replay");
    let handle = client.start(start_request)?;
    println!(
        "started runtime_id={} contract_v{}",
        handle.runtime_id, handle.active_contract_version
    );

    let send_request = SendRequest::new(
        source,
        destination,
        json!({
            "title": "SDK Example",
            "content": "hello from lxmf-sdk example"
        }),
    )
    .with_ttl_ms(30_000)
    .with_correlation_id("example-rpc-desktop-send");
    let message_id = client.send(send_request)?;
    println!("queued message_id={}", message_id.0);

    let batch = client.poll_events(None, 16)?;
    println!("polled events={} dropped={}", batch.events.len(), batch.dropped_count);

    Ok(())
}
