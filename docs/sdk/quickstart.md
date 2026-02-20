# SDK Quickstart

This quickstart covers a minimal `lxmf-sdk` client using the RPC backend.

## Prerequisites

- Rust toolchain matching `rust-toolchain.toml`
- Running `reticulumd` endpoint (default `127.0.0.1:4242`)
- Workspace checked out with `cargo check --workspace` passing

## Start `reticulumd`

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc-listen 127.0.0.1:4242
```

For secured remote bind, use token or mTLS configuration as described in:

- `docs/contracts/sdk-v2.md`
- `docs/contracts/sdk-v2-shared-instance-auth.md`

## Minimal SDK Client

```rust
use lxmf_sdk::{Client, LxmfSdk, RpcBackendClient, SdkConfig, SendRequest, StartRequest};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new(RpcBackendClient::new("127.0.0.1:4242".to_owned()));

    let start: StartRequest = StartRequest::new(SdkConfig::desktop_full_default())
        .with_requested_capability("sdk.capability.cursor_replay");

    let handle = client.start(start)?;
    println!("runtime_id={} contract={}", handle.runtime_id, handle.active_contract_version);
    Ok(())
}
```

## Send and Poll Events

```rust
let send: SendRequest = SendRequest::new(
    "example.service",
    "example.peer",
    json!({"title": "hello", "content": "sdk quickstart"}),
)
.with_ttl_ms(30_000)
.with_correlation_id("quickstart-send");

let message_id = client.send(send)?;
let batch = client.poll_events(None, 16)?;
println!("queued message_id={} events={}", message_id.0, batch.events.len());
```

## Next Steps

- Operational config patterns: `docs/sdk/configuration-profiles.md`
- Runtime lifecycle and cursor patterns: `docs/sdk/lifecycle-and-events.md`
- Capability-driven feature use: `docs/sdk/advanced-embedding.md`
