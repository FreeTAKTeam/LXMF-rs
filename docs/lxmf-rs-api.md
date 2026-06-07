# API Surface and Stability

This note summarizes which repository surfaces are intended for external use and
where stability guarantees are enforced.

## Stable Library Surfaces

The hard-break public API is crate-based, not monolithic module fan-out.

- `crates/libs/lxmf`
  - umbrella crate re-exporting the supported LXMF library entry points
- `crates/libs/lxmf-core` (published as `lxmf-wire`)
  - protocol/message/payload primitives
  - wire-field and payload-field encoding/decoding
- `crates/libs/lxmf-sdk`
  - host-facing client facade (`start/send/cancel/status/poll/configure/snapshot/shutdown/tick`)
  - capability negotiation, profile limits, lifecycle guardrails
- `crates/libs/rns-embedded-core`
  - shared embedded/runtime types for constrained hosts
- `crates/libs/rns-embedded-runtime`
  - embedded runtime facade
- `crates/libs/rns-embedded-ffi`
  - C ABI for embedded/manual-tick integrations
- `crates/libs/rns-core` (published as `reticulum-rs-core`)
  - Reticulum primitives
- `crates/libs/rns-transport` (published as `reticulum-rs-transport`)
  - transport and interface behavior
- `crates/libs/rns-rpc` (published as `reticulum-rs-rpc`)
  - daemon JSON-RPC contracts and runtime method surface (`sdk_*_v2`)
  - shared transport/auth/event contract types used by app crates
- `crates/libs/reticulum-rs`
  - umbrella crate re-exporting the supported Reticulum library entry points
- `crates/libs/test-support`
  - test-only helpers and schema/fixture validation support

`Cargo.toml` is the source of truth for active workspace members.

## Operator/App Surfaces

- `crates/apps/lxmf-cli`: operator-facing CLI over `lxmf-sdk`
- `crates/apps/reticulumd`: daemon binary hosting `reticulum-rs-rpc`
- `crates/apps/rns-tools`: diagnostics and interop helpers

App crates are not intended as stable library APIs.

## Stability Policy

- No legacy crate path compatibility guarantees (`crates/lxmf`, `crates/reticulum`, `crates/reticulum-daemon`).
- Public API drift is gated by `docs/contracts/baselines/lxmf-sdk-public-api.txt`.
- Contract behavior is governed by:
  - `docs/contracts/sdk-v2.md`
  - `docs/contracts/sdk-v2-events.md`
  - `docs/contracts/sdk-v2-errors.md`

Related references:

- `docs/contracts/sdk-v2-api-stability.md`
- `docs/contracts/support-policy.md`
- `README.md`

Published crates.io entry points:

- `lxmf`
- `lxmf-sdk`
- `lxmf-wire`
- `reticulum-rs`
- `reticulum-rs-core`
- `reticulum-rs-transport`
- `reticulum-rs-rpc`

Use the root `Cargo.toml`, crates.io badges in `README.md`, or the selected
release tag for version numbers. This page intentionally does not duplicate
mutable release versions.
