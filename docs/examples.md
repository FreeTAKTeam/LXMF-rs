# Examples

These examples target the current workspace and favor interfaces that expose
failures instead of treating command completion as delivery success.

## Run the daemon locally

Start `reticulumd` with the default local Unix RPC endpoint:

```bash
cargo run -p reticulumd --bin reticulumd
```

Enable the canonical single-endpoint ZeroMQ service for desktop SDK clients:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --zmq-rpc-endpoint tcp://127.0.0.1:9100
```

Set `RUST_LOG=reticulumd=debug,rns_transport=debug` when investigating path,
link, proof, or inbound-event behavior.

## Send through the typed ZeroMQ SDK

In another terminal, run the maintained example client:

```bash
LXMF_ZMQ_ENDPOINT=tcp://127.0.0.1:9100 \
LXMF_SOURCE=example.sender \
LXMF_DESTINATION=example.peer \
cargo run -p lxmf-sdk --example zmq_pipeline_send \
  --features zmq-pipeline-backend
```

The command reports SDK acceptance. Observe delivery and retry state through
the event stream or a later status query; acceptance alone is not proof that a
remote peer received the message.

## Use the local RPC app API

For applications that use the zero-configuration Unix endpoint:

```rust,no_run
use lxmf_sdk::app::{Client, Config, SendRequest};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), lxmf_sdk::app::Error> {
    let client = Client::rpc("unix:/tmp/lxmf-rpc.sock");
    client.runtime().start_async(Config::desktop_default()).await?;
    let accepted = client
        .messages()
        .send_async(SendRequest::new(
            "example.sender",
            "example.peer",
            json!({"title": "hello", "content": "from LXMF-rs"}),
        ))
        .await?;
    println!("accepted message_id={}", accepted.message_id);
    Ok(())
}
```

## Handle partial link fan-out

Low-level transport consumers should use the reporting variants when sending
to one or more established links:

```rust,ignore
let report = transport
    .send_to_out_links_with_report(&destination_hash, b"link payload")
    .await;

if report.matched_links == 0 {
    return Err("no active link matched the destination".into());
}
if !report.is_complete() {
    return Err(format!(
        "link fan-out incomplete: sent={} failed={} matched={}",
        report.sent_links, report.failed_links, report.matched_links
    )
    .into());
}
```

The compatibility helpers without `_with_report` remain available and log
packet-build or dispatch failures. Use the report when the caller must decide
whether to retry, surface a degraded state, or fail the operation.

## Validate public identity material

Public key bytes received from storage or a peer are untrusted input. Construct
an identity through the fallible API and preserve context at the boundary:

```rust,ignore
use rns_transport::identity::Identity;

let identity = Identity::try_new_from_slices(public_key_bytes, verifying_key_bytes)
    .map_err(|error| format!("peer {peer_id} supplied invalid identity material: {error}"))?;
```

Both keys must be exactly 32 bytes and the Ed25519 verifying key must decode.
Hex constructors likewise require the exact encoded length; malformed or
overlong input is rejected rather than truncated.

## Validate a checkout

Run focused checks while developing, then the release gate before publishing:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo test --workspace --tests
tools/scripts/check-boundaries.sh
cargo run -p xtask -- architecture-checks
cargo xtask release-check
```

Hardware-in-the-loop checks remain separately tracked for v1.0 and are not
implied by these software-only examples.
