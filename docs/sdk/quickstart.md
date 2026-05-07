# SDK Quickstart

This quickstart covers a minimal `lxmf-sdk` client using the RPC backend.

## Prerequisites

- Rust toolchain matching `rust-toolchain.toml`
- Running `reticulumd` endpoint (default `127.0.0.1:4242`)
- Workspace checked out with `cargo check --workspace` passing

## Start `reticulumd`

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4242
```

For local app integration, expose a Unix socket alongside the TCP listener:

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4242 --rpc-unix /tmp/lxmf-rpc.sock
```

Then connect with `Client::rpc("unix:/tmp/lxmf-rpc.sock")`.

For secured remote bind, use token or mTLS configuration as described in:

- `docs/contracts/sdk-v2.md`
- `docs/contracts/sdk-v2-shared-instance-auth.md`

## Minimal SDK Client

The app-facing path is event-driven: subscribe once, then handle typed events from the stream.

```rust
use lxmf_sdk::app::{Client, Config, EventKind, SendRequest, SubscriptionStart};
use serde_json::json;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), lxmf_sdk::app::Error> {
    let client = Client::rpc("127.0.0.1:4242");
    let handle = client.runtime().start_async(Config::desktop_default()).await?;
    println!("runtime_id={}", handle.runtime_id);

    let mut events = client.events().subscribe(SubscriptionStart::Tail)?;
    let receipt = client.messages().send_async(
        SendRequest::new(
            "example.service",
            "example.peer",
            json!({"title": "hello", "content": "sdk quickstart"}),
        )
        .with_ttl_ms(30_000)
        .with_correlation_id("quickstart-send"),
    )
    .await?;
    println!("queued message_id={}", receipt.message_id);

    while let Some(event) = events.next().await.transpose()? {
        match event.kind {
            EventKind::InboundMessageReceived => {
                println!("received inbound message event");
            }
            EventKind::MessageDelivered
                if event.metadata.message_id.as_deref() == Some(receipt.message_id.as_str()) =>
            {
                println!("message delivered");
                break;
            }
            EventKind::StreamGapDetected(gap) => {
                eprintln!("stream gap requires recovery: {:?}", gap);
                break;
            }
            _ => {}
        }
    }
    Ok(())
}
```

## Send and Poll Events

`messages().send_async(...)` returns message acceptance. Delivery, retry, inbound, and gap state
arrives through `events().subscribe(...)`; do not add a one-second app polling loop.

### Low-Level Cursor Recovery

`poll_events(cursor, max)` is still part of the contract, but normal apps should not loop on it.
Use it for explicit recovery, deterministic tests, manual embedded hosts, or diagnostics that need
direct cursor control.

## Next Steps

- Operational config patterns: `docs/sdk/configuration-profiles.md`
- Runtime lifecycle and cursor patterns: `docs/sdk/lifecycle-and-events.md`
- Capability-driven feature use: `docs/sdk/advanced-embedding.md`
