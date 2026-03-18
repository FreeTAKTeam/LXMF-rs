# Contributing to LXMF-rs

This repository has a large documentation surface because some docs are also
contract inputs, schema fixtures, or release gates. Before deleting or moving
anything under `docs/`, check `docs/README.md` so you do not remove a file that
CI or tests depend on.

## Start Here

1. Read `README.md` for the workspace overview.
2. Read `docs/README.md` for the docs map and retention rules.
3. Run `make bootstrap` to install the expected Rust toolchain and local tools.

## Workspace Shape

Active workspace members are defined in `Cargo.toml`.

- Libraries:
  - `lxmf-core`
  - `lxmf-grpc-client`
  - `lxmf-sdk`
  - `rns-core`
  - `rns-embedded-core`
  - `rns-embedded-ffi`
  - `rns-embedded-runtime`
  - `rns-rpc`
  - `rns-transport`
  - `test-support`
- Applications:
  - `lxmf-cli`
  - `reticulumd`
  - `rns-tools`
- Workspace tooling:
  - `xtask`

These directories exist but are not active workspace members:

- `crates/internal/*`: retained legacy crates
- `crates/libs/lxmf-router`
- `crates/libs/lxmf-runtime`

Do not assume every crate directory is live just because it exists on disk.

## Bootstrap

Recommended:

```bash
make bootstrap
```

Direct script form:

```bash
./tools/scripts/bootstrap-dev.sh
```

Verification-only mode:

```bash
./tools/scripts/bootstrap-dev.sh --check --skip-smoke
```

## Common Local Commands

Fast confidence:

```bash
cargo check --workspace --all-targets
cargo test --workspace
./tools/scripts/check-boundaries.sh
```

Full local gate:

```bash
cargo xtask ci
```

Release-oriented gate:

```bash
cargo xtask release-check
```

Target one binary when you do not need the whole workspace:

```bash
make check-bin PKG=lxmf-cli BIN=lxmf-cli
make run-bin PKG=rns-tools BIN=rngrpc ARGS="--help"
```

## Documentation Rules

- Prefer updating an existing source-of-truth doc over adding a parallel note.
- Use relative repository paths in docs and examples. Do not commit machine-
  specific absolute paths.
- If a new doc supersedes an old one, delete the old doc in the same PR after
  fixing inbound links.
- Treat `docs/contracts/`, `docs/schemas/`, and `docs/fixtures/` as code-adjacent
  artifacts. Many are validated by CI.
- Keep `README.md` short and contributor-oriented. Deep operator guidance should
  live in `docs/runbooks/` or package-specific `README.md` files.

## When Docs Must Change

Update docs when you change:

- public SDK or RPC behavior
- schema or fixture shape
- contributor bootstrap or validation commands
- operator workflows that are described in `docs/runbooks/`
- support, migration, or compatibility guarantees

Useful entry points:

- `docs/README.md`
- `docs/sdk/README.md`
- `docs/runbooks/release-readiness.md`
- `docs/contracts/support-policy.md`

## Pull Requests

Before opening a PR, run the narrowest validation that proves your change.
For contract, schema, or contributor-workflow changes, prefer `cargo xtask ci`
or the specific `xtask` check that covers the affected area.

If you touch docs:

- remove or update stale links
- keep docs consistent with the actual workspace layout
- call out any intentionally retained historical docs in the PR summary

## Security

Do not open public issues for suspected vulnerabilities. Follow
`SECURITY.md` and `.github/SECURITY.md`.
