# LXMF-rs Monorepo

Rust monorepo for LXMF and Reticulum with strict library/app boundaries and enterprise quality gates.

## Repository Layout

```text
LXMF-rs/
├── crates/
│   ├── libs/
│   │   ├── lxmf-core/
│   │   ├── lxmf-sdk/
│   │   ├── rns-core/
│   │   ├── rns-transport/
│   │   ├── rns-rpc/
│   │   └── test-support/
│   ├── apps/
│   │   ├── lxmf-cli/
│   │   ├── reticulumd/
│   │   └── rns-tools/
└── docs/
    ├── adr/
    ├── architecture/
    ├── contracts/
    ├── migrations/
    └── runbooks/
├── tools/
│   └── scripts/
├── xtask/
└── target/

Note: legacy migration-only implementation crates are retained under
`crates/internal/` and are excluded from the active workspace graph.
```

## Public Crates

- `lxmf-core`: message/payload/identity primitives.
- `lxmf-sdk`: host-facing client API (`start/send/cancel/status/configure/poll/snapshot/shutdown`).
- `rns-embedded-runtime`: node-centric embedded runtime facade with lifecycle, event, and managed `std` driver support.
- `rns-embedded-ffi`: C ABI for embedded/manual-tick compatibility and the v1 node-centric API.
- `rns-core`: Reticulum cryptographic and packet primitives.
- `rns-transport`: transport + iface + receipt/resource API.
- `rns-rpc`: RPC request/response/event contracts and bridges.

## Build and Validation

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo doc --workspace --no-deps
./tools/scripts/check-boundaries.sh
```

or via `xtask`:

```bash
cargo xtask ci
cargo xtask release-check
cargo xtask api-diff
cargo xtask python-impl-bench-compare
```

Cross-language protocol benchmark reports are written to
`target/criterion/python-impl-compare.txt` and compare Rust core paths to the
installed Python `RNS` and `LXMF` implementations. Benchmark configuration
lives in `tools/benchmarks/python_impl.toml`, and the operating runbook is
`docs/runbooks/python-impl-benchmarking.md`.

## Developer Bootstrap

One-command local setup:

```bash
make bootstrap
```

Direct script form:

```bash
./tools/scripts/bootstrap-dev.sh
```

Verification-only mode (used by CI):

```bash
./tools/scripts/bootstrap-dev.sh --check --skip-smoke
```

## Binaries

- `lxmf-cli`
- `reticulumd`
- `rncp`, `rnid`, `rnir`, `rnodeconf`, `rnpath`, `rnpkg`, `rnprobe`, `rnsd`, `rnstatus`, `rnx`

Run examples:

```bash
cargo run -p lxmf-cli -- --help
cargo run -p reticulumd -- --help
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

## Contracts and Runbooks

- Compatibility contract: `docs/contracts/compatibility-contract.md`
- Compatibility matrix: `docs/contracts/compatibility-matrix.md`
- Third-party compatibility kit: `docs/contracts/third-party-compatibility-kit.md`
- Support and LTS policy: `docs/contracts/support-policy.md`
- Extension registry: `docs/contracts/extension-registry.md`
- RPC contract: `docs/contracts/rpc-contract.md`
- Payload contract: `docs/contracts/payload-contract.md`
- Release readiness: `docs/runbooks/release-readiness.md`
- Release scorecard process: `docs/architecture/overview.md`

## SDK Guide

- Guide index: `docs/sdk/README.md`
- Quickstart: `docs/sdk/quickstart.md`
- Profiles/configuration: `docs/sdk/configuration-profiles.md`
- Config cookbook: `docs/runbooks/sdk-config-cookbook.md`
- Lifecycle/events: `docs/sdk/lifecycle-and-events.md`
- Advanced embedding: `docs/sdk/advanced-embedding.md`

## Embedded Node FFI

- Header: `crates/libs/rns-embedded-ffi/include/rns_embedded_ffi.h`
- Guide and example: `crates/libs/rns-embedded-ffi/README.md`
- Stable core contract: lifecycle, status, capability probe, send/broadcast, subscriptions, structured errors
- Compatibility surface: legacy manual tick, raw wire ingress/egress, low-level queueing
- Extension surface: numeric extension IDs validated by `docs/fixtures/embedded/public-node-api-v1/extension-ids.json`
- `v1` node-centric API: `rns_embedded_v1_node_new/start/stop/restart/get_status/send/broadcast/set_log_level/subscribe_events`
- legacy compatibility API remains available for manual tick, raw wire ingress/egress, and low-level queueing

## Governance

- Governance docs: `SECURITY.md`
- Security policy: `SECURITY.md`
- Code ownership: `.github/CODEOWNERS`

## License

MIT
