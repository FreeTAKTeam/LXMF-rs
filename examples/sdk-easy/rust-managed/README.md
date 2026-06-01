# Rust Managed Easy-Mode Example

This example is the copy-pasteable Rust path for normal app teams. It uses the
app-facing `lxmf_sdk::app::Client` surface instead of low-level polling or
manual transport code.

Run a local daemon first:

```powershell
cargo run -p reticulumd --bin reticulumd
```

Then run the example:

```powershell
$env:LXMF_RPC_ENDPOINT = "unix:/tmp/lxmf-rpc.sock"
$env:LXMF_SOURCE = "example.app"
$env:LXMF_DESTINATION = "example.peer"
cargo run --manifest-path examples/sdk-easy/rust-managed/Cargo.toml
```

The flow follows the SDK app v1 conformance scenarios:

- `lifecycle.start_stop_restart`
- `events.delivery_ordering`
- `delivery.queue_pressure`
- `timeout.poll_timeout`
- `connectivity.reconnect_recovery`
- `errors.typed_mapping`
- `compatibility.unknown_additive`
