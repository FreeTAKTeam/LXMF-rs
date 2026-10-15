# SDK Quickstart

This quickstart covers the typed ZeroMQ backend for desktop/service clients,
with Unix and TCP RPC compatibility entry points.

## Prerequisites

- Rust 1.88 or newer; repository builds use `rust-toolchain.toml`.
- A running `reticulumd` endpoint. The minimal client below uses an explicitly
  enabled local ZeroMQ listener.
- An actual peer destination for delivery; `LXMF_DESTINATION` in the example
  must identify a reachable peer, not a placeholder.

## Start `reticulumd`

```bash
cargo run -p reticulumd --bin reticulumd
```

Then connect with `Client::rpc("unix:/tmp/lxmf-rpc.sock")`. Local Unix RPC is
the zero-configuration daemon endpoint and remains supported for compatibility.

For explicit TCP development, opt in with `--rpc`:

```bash
cargo run -p reticulumd --bin reticulumd -- --rpc 127.0.0.1:4242
```

Remote TCP binds (`0.0.0.0`, non-loopback IPv4, or non-loopback IPv6) are refused
unless remote token auth is already configured in the persisted SDK runtime config or
mTLS client authentication is configured at startup with `--rpc-tls-client-ca`.
Use loopback TCP only for local development.

## Canonical ZeroMQ Backend

ZeroMQ is the preferred high-throughput SDK transport for desktop integrations.
It is compiled into the default `reticulumd` build, but the listener is
explicitly enabled so an unused network endpoint is never opened implicitly:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --zmq-rpc-endpoint tcp://127.0.0.1:9100
```

The SDK and daemon both include ZeroMQ support in their default features.
For an application outside this workspace, add:

```toml
[dependencies]
lxmf-sdk = "0.14.0"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "time"] }
tokio-stream = "0.1"
```

If disabling default features, retain `std`, `sdk-async` and
`zmq-pipeline-backend`. The app-facing constructor is:

```rust
use lxmf_sdk::app::Client;

let client = Client::zmq_local("tcp://127.0.0.1:9100")?;
```

Use loopback endpoints for local testing. Remote ZeroMQ endpoints require explicit token auth; the
backend rejects remote endpoints without it. `poll_events` remains the authoritative event recovery
API even when ZeroMQ event wakeups are enabled.

The typed `ZmqPipelineBackendClient` path covers the core lifecycle and
delivery methods plus identity list/activate/import/export, identity announce,
presence list, identity resolve, contact update/list, identity bootstrap,
typed peer directory through `ZmqPipelineBackendClient::peer_directory`,
typed peer-ready setup through `ZmqPipelineBackendClient::workflow_peer_ready`,
operation registry, envelope execution, and typed durable direct-chat history
through `ZmqPipelineBackendClient::list_message_history`. History requests use
the `app.message.history.list` SDK envelope path and preserve one-to-one
`peer_id`/`conversation_id` filters, `include_receipts`, links in message
bodies, basic LXMF fields, and restart pagination cursors. Peer directory
merges saved contacts and announce-derived presence over typed ZeroMQ SDK calls,
preserving display names, callsigns, REM capability flags, and RCH announce-slot
metadata. Peer-ready setup preserves display names, callsigns, and capability
metadata while optionally announcing before use. Runtime destination
queries can use `ZmqPipelineBackendClient::local_delivery_destination_hash`;
operation-driven clients can still use `app.delivery.destination_hash` through
SDK envelope execution. Burst sends can use
`ZmqPipelineBackendClient::send_batch` for typed ordered per-message
acceptance and rejection results; operation-driven clients can use
`app.delivery.send_batch` through SDK envelope execution. Direct-chat
cancellation can use either `sdk_cancel_message_v2` via
`ZmqPipelineBackendClient::cancel` or `app.delivery.cancel` through SDK
envelope execution, preserving `Accepted`, `AlreadyTerminal`, `NotFound`, and
`TooLateToCancel` results. Delivery status follows negotiated receipt semantics
on the ZeroMQ path: `sent` is terminal until
`sdk.capability.receipt_terminality` is negotiated, then `delivered` is the
terminal receipt state. Status snapshots also preserve daemon-reported
retry-attempt counts and reason codes so REM/RCH can surface restart/retry
state without raw RPC status calls.

Run the example client:

```bash
LXMF_ZMQ_ENDPOINT=tcp://127.0.0.1:9100 \
cargo run -p lxmf-sdk --example zmq_pipeline_send --features zmq-pipeline-backend
```

For transport measurements, use the versioned methodology and complete
results in the [performance guide](../performance.md). Benchmark/stress
workloads are separate from this application startup path.

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
Set `LXMF_DESTINATION` to the peer destination before running the application.
The observation window is bounded, and terminality comes from the typed status
rather than assuming every deployment waits for a delivery receipt.

```rust
use lxmf_sdk::app::{Client, Config, EventKind, SendRequest, SubscriptionStart};
use serde_json::json;
use std::io;
use tokio::time::{timeout, Duration};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), io::Error> {
    // Format app errors only at this executable boundary; control flow stays typed.
    let report = |error: lxmf_sdk::app::Error| {
        io::Error::other(format!("{}: {}", error.code.as_str(), error.message))
    };
    let destination = std::env::var("LXMF_DESTINATION").map_err(io::Error::other)?;
    let client = Client::zmq_local("tcp://127.0.0.1:9100").map_err(io::Error::other)?;
    let handle = client
        .runtime()
        .start_async(Config::desktop_default())
        .await
        .map_err(report)?;
    println!("runtime_id={}", handle.runtime_id);

    let mut events = client
        .events()
        .subscribe(SubscriptionStart::Tail)
        .map_err(report)?;
    let receipt = client
        .messages()
        .send_async(
            SendRequest::new(
                "example.service",
                destination,
                json!({"title": "hello", "content": "sdk quickstart"}),
            )
            .with_ttl_ms(30_000)
            .with_correlation_id("quickstart-send")
            .with_delivery_method("direct")
            .with_stamp_cost(8)
            .with_include_ticket(true)
            .with_try_propagation_on_fail(true),
        )
        .await
        .map_err(report)?;
    println!("queued message_id={}", receipt.message_id);

    let observation = timeout(Duration::from_secs(35), async {
        while let Some(event) = events.next().await.transpose().map_err(report)? {
            if let EventKind::StreamGapDetected(gap) = &event.kind {
                return Err(io::Error::other(format!(
                    "reconciliation required: {gap:?}"
                )));
            }
            if event.metadata.message_id.as_deref() == Some(receipt.message_id.as_str()) {
                if let Some(status) = client
                    .messages()
                    .status_async(receipt.message_id.clone())
                    .await
                    .map_err(report)?
                {
                    if status.terminal {
                        println!("terminal delivery state={:?}", status.state);
                        return Ok::<(), io::Error>(());
                    }
                }
            }
        }
        Err(io::Error::other(
            "event stream ended before a terminal status",
        ))
    })
    .await;
    drop(events);
    client
        .runtime()
        .stop_async(lxmf_sdk::ShutdownMode::Graceful)
        .await
        .map_err(report)?;
    observation.map_err(io::Error::other)??;
    Ok(())
}
```

Unix RPC users can replace the constructor with
`Client::rpc("unix:/tmp/lxmf-rpc.sock")`. For production recovery, retain
correlation/idempotency information and reconcile typed status after a stream
gap or uncertain transport failure; see [delivery states](delivery-states.md)
and [error handling](error-handling.md).

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
