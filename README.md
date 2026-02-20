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
```

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
- RPC contract: `docs/contracts/rpc-contract.md`
- Payload contract: `docs/contracts/payload-contract.md`
- Release readiness: `docs/runbooks/release-readiness.md`

## SDK Guide

- Guide index: `docs/sdk/README.md`
- Quickstart: `docs/sdk/quickstart.md`
- Profiles/configuration: `docs/sdk/configuration-profiles.md`
- Config cookbook: `docs/runbooks/sdk-config-cookbook.md`
- Lifecycle/events: `docs/sdk/lifecycle-and-events.md`
- Advanced embedding: `docs/sdk/advanced-embedding.md`

## Governance

- Governance docs: `SECURITY.md`
- Security policy: `SECURITY.md`
- Code ownership: `.github/CODEOWNERS`

## License

MIT
