# SDK Quickstart

This quickstart covers a minimal `lxmf-sdk` client using the RPC backend.

## Prerequisites

- Rust toolchain matching `rust-toolchain.toml`
- Running `reticulumd` endpoint (default `unix:/tmp/lxmf-rpc.sock`)
- Workspace checked out with `cargo check --workspace` passing

## Start `reticulumd`

```bash
cargo run -p reticulumd --bin reticulumd
```

Then connect with `Client::rpc("unix:/tmp/lxmf-rpc.sock")`.

For explicit TCP development, opt in with `--rpc`:

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4242
```

Remote TCP binds (`0.0.0.0`, non-loopback IPv4, or non-loopback IPv6) are refused
unless remote token auth is already configured in the persisted SDK runtime config or
mTLS client authentication is configured at startup with `--rpc-tls-client-ca`.
Use loopback TCP only for local development.

For first-run token-authenticated TCP, put the shared secret in an environment variable
and point `reticulumd` at the variable name:

```bash
export LXMF_RPC_TOKEN_SECRET='replace-with-a-generated-secret'
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 0.0.0.0:4242 \
  --rpc-token-issuer example-issuer \
  --rpc-token-audience example-audience \
  --rpc-token-secret-env LXMF_RPC_TOKEN_SECRET
```

Do not pass token secrets directly as command-line arguments. After startup,
the token settings are validated as SDK runtime config and satisfy the remote
bind guard.

For secured remote bind details, use token or mTLS configuration as described in:

- `docs/sdk/remote-mtls.md`
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
    let client = Client::rpc("unix:/tmp/lxmf-rpc.sock");
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
- Remote mTLS example: `docs/sdk/remote-mtls.md`
- Runtime lifecycle and cursor patterns: `docs/sdk/lifecycle-and-events.md`
- Polling migration: `docs/sdk/polling-to-events-migration.md`
- Capability-driven feature use: `docs/sdk/advanced-embedding.md`
