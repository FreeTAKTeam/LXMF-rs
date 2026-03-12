# grpc-hello-rust

Small standalone example showing how an external Rust project can consume the
in-repo `lxmf-grpc-client` crate.

## Run

Start a local daemon first:

```bash
cargo run -p reticulumd --bin reticulumd -- \
  --rpc 127.0.0.1:4243 \
  --grpc 127.0.0.1:50051 \
  --db reticulum.db
```

Then run the example:

```bash
cargo run --manifest-path examples/grpc-hello-rust/Cargo.toml
```

If token auth is enabled:

```bash
LXMF_GRPC_ENDPOINT=https://127.0.0.1:50051 \
LXMF_GRPC_BEARER_TOKEN=<token> \
cargo run --manifest-path examples/grpc-hello-rust/Cargo.toml
```
