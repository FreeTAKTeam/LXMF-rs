# 2026 Aggressive Platform Split Migration

Date: 2026-02-19

## Summary

Repository topology moved to layered public crates in `crates/libs/*`, app
binaries in `crates/apps/*`, and short-lived legacy implementation crates in
`crates/internal/*` during migration. The legacy/internal crates and
transitional router/runtime stubs have since been retired from the repository.

Historical naming note: this migration record keeps the workspace directory
names used during the split. The published crates.io names are `lxmf-wire`,
`reticulum-rs-core`, `reticulum-rs-transport`, and `reticulum-rs-rpc`.

## Breaking Changes

1. Old crate paths under `crates/lxmf`, `crates/reticulum`, and `crates/reticulum-daemon` were removed.
2. Stable interfaces are now exposed through:
   - `lxmf-core` (published as `lxmf-wire`)
   - `lxmf-sdk`
   - `rns-core` (published as `reticulum-rs-core`)
   - `rns-transport` (published as `reticulum-rs-transport`)
   - `rns-rpc` (published as `reticulum-rs-rpc`)
3. `lxmf-router` and `lxmf-runtime` were transitional stubs only and are now
   removed from the active repository surface.
4. Binary crates moved to:
   - `crates/apps/lxmf-cli`
   - `crates/apps/reticulumd`
   - `crates/apps/rns-tools`
5. Python interop harness scripts are no longer owned in this repository.

## Required Consumer Actions

1. Update workspace path dependencies to new crate/package names.
2. Use new docs locations:
   - contracts: `docs/contracts/*`
   - release runbooks: `docs/runbooks/*`
3. Use `cargo xtask`/`make` Rust-only gates in local automation.

## Validation

```bash
cargo check --workspace --all-targets
cargo test --workspace
./tools/scripts/check-boundaries.sh
```
