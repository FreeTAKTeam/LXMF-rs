# Bruno Collections

This folder contains git-committable Bruno assets for exploring `reticulumd`.

Current contents:

- `reticulumd-jsonrpc-compat/`
  - local-ready JSON-RPC collection for the browser-safe `/rpc/json` endpoint
- `reticulumd-grpc/`
  - gRPC collection shell with reflection/proto guidance for Bruno's gRPC UI

## Why JSON-RPC First

Bruno can drive both HTTP and gRPC, but the JSON-RPC compatibility surface is the
most portable starting point for a committed collection because it is plain HTTP
and maps cleanly to curated request bodies.

For gRPC in Bruno, point Bruno's gRPC request UI at the live daemon using server
reflection, or open the dedicated gRPC collection shell:

- `bruno/reticulumd-grpc`

- host: `127.0.0.1:50051`
- reflection: enabled by the daemon

Reference docs:

- `docs/grpc-getting-started.md`
- `docs/runbooks/grpc.md`

## Start the Daemon

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 127.0.0.1:4243 \
  --grpc 127.0.0.1:50051 \
  --db reticulum.db
```

Then open either:

- `bruno/reticulumd-jsonrpc-compat`
- `bruno/reticulumd-grpc`
