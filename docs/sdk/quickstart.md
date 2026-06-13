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

HTTP/Unix RPC is still the default SDK path.

For explicit TCP development, opt in with `--rpc`:

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4242
```

Remote TCP binds (`0.0.0.0`, non-loopback IPv4, or non-loopback IPv6) are refused
unless remote token auth is already configured in the persisted SDK runtime config or
mTLS client authentication is configured at startup with `--rpc-tls-client-ca`.
Use loopback TCP only for local development.

## ZeroMQ Backend

The ZeroMQ backend is parallel and opt-in. It is the preferred SDK transport
for high-throughput local integrations and the REM/RCH 0.4.0 compatibility
track:

```toml
lxmf-sdk = { path = "crates/libs/lxmf-sdk", features = ["zmq-pipeline-backend"] }
```

```rust
use lxmf_sdk::{Client, ZmqPipelineBackendClient, ZmqPipelineBackendConfig};

let backend = ZmqPipelineBackendClient::new(ZmqPipelineBackendConfig::local_tcp(
    "tcp://127.0.0.1:9100",
    "tcp://127.0.0.1:9101",
))?;
let client = Client::new(backend);
```

Use loopback endpoints for local testing. Remote ZeroMQ endpoints require explicit token auth; the
backend rejects remote endpoints without it. `poll_events` remains the authoritative event recovery
API even when ZeroMQ event wakeups are enabled.

The typed `ZmqPipelineBackendClient` path covers the core lifecycle and
delivery methods plus identity list/activate/import/export, identity announce,
presence list, identity resolve, contact update/list, identity bootstrap,
operation registry, and envelope execution. Direct-chat history and runtime
destination queries should use `app.message.history.list` and
`app.delivery.destination_hash` through the SDK envelope path instead of
constructing raw RPC envelopes. Delivery status follows negotiated receipt
semantics on the ZeroMQ path: `sent` is terminal until
`sdk.capability.receipt_terminality` is negotiated, then `delivered` is the
terminal receipt state.

Run the example client:

```bash
LXMF_ZMQ_COMMAND=tcp://127.0.0.1:9100 \
LXMF_ZMQ_RESPONSE=tcp://127.0.0.1:9101 \
cargo run -p lxmf-sdk --example zmq_pipeline_send --features zmq-pipeline-backend
```

Start `reticulumd` with HTTP and ZeroMQ enabled for a local stress comparison:

```powershell
cargo run -p reticulumd --features zmq-pipeline-rpc --bin reticulumd -- `
  --rpc 127.0.0.1:4242 `
  --zmq-rpc-command tcp://127.0.0.1:9100 `
  --db target/stress-pr199/reticulumd.db `
  --identity target/stress-pr199/reticulumd.identity
```

Run the ignored HTTP-vs-ZeroMQ stress comparison:

```powershell
$env:LXMF_STRESS_HTTP_RPC='127.0.0.1:4242'
$env:LXMF_STRESS_ZMQ_COMMAND='tcp://127.0.0.1:9100'
$env:LXMF_STRESS_ZMQ_RESPONSE='tcp://127.0.0.1:9101'
$env:LXMF_STRESS_ITERATIONS='1000'
cargo test -p lxmf-sdk --features zmq-pipeline-backend --test transport_stress -- --ignored --nocapture --test-threads=1
```

The stress output is a terse two-line timing report:

```text
transport_stress op=snapshot iterations=1000 http_ms=... http_avg_us=... http_ops=... zmq_ms=... zmq_avg_us=... zmq_ops=... zmq_http_ratio=...
transport_stress op=poll_events iterations=1000 http_ms=... http_avg_us=... http_ops=... zmq_ms=... zmq_avg_us=... zmq_ops=... zmq_http_ratio=...
```

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
        .with_correlation_id("quickstart-send")
        .with_delivery_method("direct")
        .with_stamp_cost(8)
        .with_include_ticket(true)
        .with_try_propagation_on_fail(true),
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

## Easy-Mode Golden Paths

For copy-pasteable app starts, use the checked examples:

- Rust managed app: `examples/sdk-easy/rust-managed`
- Kotlin mobile wrapper shape: `examples/sdk-easy/kotlin-mobile`
- First-party Kotlin wrapper source: `wrappers/kotlin-mobile`

Both examples are anchored to the SDK app v1 conformance manifest at
`docs/fixtures/sdk-app-v1/manifest.json`. Low-level integrations should migrate
through `docs/sdk/migration-to-easy.md` before adding wrapper-specific behavior.

## Send and Poll Events

`messages().send_async(...)` returns message acceptance. Delivery, retry, inbound, and gap state
arrives through `events().subscribe(...)`; do not add a one-second app polling loop.

`SendRequest` also carries per-message delivery options for the normal send path:

- `with_delivery_method("direct" | "propagated" | "paper")`
- `with_stamp_cost(cost)`
- `with_include_ticket(true)`
- `with_try_propagation_on_fail(true)`

### Low-Level Cursor Recovery

`poll_events(cursor, max)` is still part of the contract, but normal apps should not loop on it.
Use it for explicit recovery, deterministic tests, manual embedded hosts, or diagnostics that need
direct cursor control.

## Next Steps

- Operational config patterns: `docs/sdk/configuration-profiles.md`
- Easy-mode migration: `docs/sdk/migration-to-easy.md`
- Remote mTLS example: `docs/sdk/remote-mtls.md`
- Runtime lifecycle and cursor patterns: `docs/sdk/lifecycle-and-events.md`
- Polling migration: `docs/sdk/polling-to-events-migration.md`
- Capability-driven feature use: `docs/sdk/advanced-embedding.md`
