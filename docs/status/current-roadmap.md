# Current Roadmap Status

Last updated: 2026-04-02

This document is the current source of truth for repository-wide delivery
status. Update this file first when parity status, release confidence, or the
active execution order changes.

Related documents:

- Execution board: `docs/plans/2026-03-19-python-compatibility-execution-board.md`
- Numbered compatibility backlog: `docs/plans/2026-03-18-rust-python-compat-issue-list.md`
- LXMF parity snapshot: `docs/plans/lxmf-parity-matrix.md`
- Reticulum parity snapshot: `docs/plans/reticulum-parity-matrix.md`

## Current Summary

- Build, test, clippy, and default boundary/module-size gates are green.
- `cargo run -p xtask -- architecture-checks` is green again after tightening the
  strict boundary manifest check to ignore active crate package names.
- The repository is not blocked by broken builds. The main blockers are parity
  gaps, migration-era duplication, oversized active modules, and CI/doc drift.

## What Is True Now

### Landed Baseline

The following compatibility foundation work is on `main` and should be treated
as the current baseline:

- buffer writer parity (`#110`)
- buffer callback parity (`#111`)
- resource lifecycle truth and generic-resource handling (`#112`)
- daemon receipt semantics for resource-backed sends (`#113`)
- honor LXMF delivery modes in the `reticulumd` bridge (`#114`)
- path tag lifetime parity (`#115`)

This means older planning notes that say `reticulumd` ignores requested LXMF
delivery modes are stale. Delivery-mode handling is no longer an open baseline
gap, even though deeper propagation-router parity remains open.

### Still Open

- Rust/Python live interop is not yet a credible gate. The harness-backed tests
  in `crates/apps/reticulumd/tests/python_compat_matrix.rs` and
  `crates/apps/lxmf-cli/tests/python_lxmd_remote_relay.rs` remain ignored unless
  external setup is provided.
- Propagation-router behavior is still partial relative to Python LXMF.
- Stamp, ticket, and propagation-stamp semantics are still partial.
- Peer/router/runtime parity remains partial.
- Reticulum interface breadth is still narrower than the Python reference.
- `rns-tools` still contains several stub binaries.
- Large active files remain a maintenance bottleneck despite module-size policy.

## Active Execution Order

1. Keep architecture and boundary gates trustworthy.
2. Make one Rust/Python interop path run in automation without ignored tests.
3. Burn down the largest active module hotspots:
   - `crates/libs/lxmf-sdk/src/app/node.rs`
   - `crates/apps/rns-tools/src/bin/rnx.rs`
   - `crates/libs/rns-rpc/src/rpc/daemon.rs`
   - `crates/apps/lxmf-cli/src/bin/lxmd.rs`
4. Reconcile legacy duplication under `crates/internal/*-legacy`.
5. Align `README.md`, `docs/runbooks/release-readiness.md`, and GitHub CI with
   the same definition of "green".

## Update Rules

- Update this file in the same PR that changes project-wide status claims.
- When a historical planning note disagrees with this file, treat the planning
  note as stale until it is refreshed.
- Do not mark parity items as complete here unless the behavior is implemented
  in active workspace code and backed by non-ignored evidence.
