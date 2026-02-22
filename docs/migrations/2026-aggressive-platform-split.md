# 2026 Aggressive Platform Split Migration

Date: 2026-02-19

## Summary

Repository topology moved to layered public crates in `crates/libs/*`, app binaries in `crates/apps/*`, and legacy implementation crates in `crates/internal/*` during migration.

## Breaking Changes

1. Old crate paths under `crates/lxmf`, `crates/reticulum`, and `crates/reticulum-daemon` were removed.
2. Stable interfaces are now exposed through:
   - `lxmf-core`
   - `lxmf-sdk`
   - `rns-core`
   - `rns-transport`
   - `rns-rpc`
3. `lxmf-router` and `lxmf-runtime` are retained as transitional stubs only and are excluded from
   the active workspace graph and stable public contract surface.
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
