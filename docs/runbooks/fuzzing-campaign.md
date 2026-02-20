# Fuzzing Campaign Runbook

This runbook defines parser/envelope fuzz coverage for SDK v2.5.

## Covered targets

- `crates/libs/rns-rpc/fuzz/fuzz_targets/rpc_frame_envelope.rs`
  - msgpack frame decode for `RpcRequest` and `RpcResponse`
  - HTTP response body extraction and JSON RPC response parsing
- `crates/libs/lxmf-sdk/fuzz/fuzz_targets/sdk_json_envelope.rs`
  - JSON envelope decode for `EventBatch`, `SdkEvent`, `SdkError`, `RuntimeSnapshot`, `DeliverySnapshot`, and `ConfigPatch`

## CI gate (fast)

```bash
cargo run -p xtask -- sdk-fuzz-check
```

The CI gate performs:
- fuzz target compile checks for both fuzz crates
- deterministic fuzz-smoke parser runs in `rns-rpc` and `lxmf-sdk`

## Long campaign (recommended pre-release)

Install `cargo-fuzz` once:

```bash
cargo install cargo-fuzz
```

Run a campaign with minimum runtime of 300 seconds per target:

```bash
FUZZ_MAX_TOTAL_TIME=300 ./tools/scripts/fuzz-campaign.sh
```

Policy:
- no crashes, panics, or sanitizer failures are acceptable
- any reproducer must be committed as regression test before release
