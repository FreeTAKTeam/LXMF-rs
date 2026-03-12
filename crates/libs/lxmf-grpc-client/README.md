# lxmf-grpc-client

`lxmf-grpc-client` is the first official Rust client crate for the LXMF gRPC API.

It compiles client stubs directly from the workspace proto tree and exposes:

- generated service clients for runtime, delivery, command, admin, topics, attachments, events, identity, markers, and peers
- a small `LxmfGrpcClient` wrapper that builds a shared gRPC channel
- optional bearer-token metadata injection
- optional TLS/mTLS client settings

## Quick Start

```rust
use lxmf_grpc_client::lxmf::runtime::v1::GetSnapshotRequest;
use lxmf_grpc_client::LxmfGrpcClient;

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let client = LxmfGrpcClient::connect("http://127.0.0.1:50051").await?;
let snapshot = client
    .runtime()
    .get_snapshot(GetSnapshotRequest { include_counts: true })
    .await?
    .into_inner();

println!("runtime={}", snapshot.runtime_id);
# Ok(())
# }
```

## Smoke Example

Run against a local daemon:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 127.0.0.1:4243 \
  --grpc 127.0.0.1:50051 \
  --db reticulum.db
```

Then:

```bash
LXMF_GRPC_ENDPOINT=http://127.0.0.1:50051 \
cargo run -p lxmf-grpc-client --example smoke
```

If token auth is enabled:

```bash
LXMF_GRPC_ENDPOINT=https://127.0.0.1:50051 \
LXMF_GRPC_BEARER_TOKEN=<token> \
cargo run -p lxmf-grpc-client --example smoke
```
