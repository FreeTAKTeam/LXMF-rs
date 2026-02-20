# LXMF Rust API (SDK v2.5)

## Stable Crate Surfaces

The hard-break public API is crate-based, not monolithic module fan-out.

- `crates/libs/lxmf-core`
  - protocol/message/payload primitives
  - wire-field and payload-field encoding/decoding
- `crates/libs/lxmf-sdk`
  - host-facing client facade (`start/send/cancel/status/poll/configure/snapshot/shutdown/tick`)
  - capability negotiation, profile limits, lifecycle guardrails
- `crates/libs/rns-rpc`
  - daemon RPC contracts and runtime method surface (`sdk_*_v2`)
  - shared transport/auth/event contract types used by app crates

## Operator/App Surfaces

- `crates/apps/lxmf-cli`: operator-facing CLI over `lxmf-sdk`
- `crates/apps/reticulumd`: daemon binary hosting `rns-rpc`
- `crates/apps/rns-tools`: diagnostics and interop helpers

App crates are not intended as stable library APIs.

## API Policy

- No legacy crate path compatibility guarantees (`crates/lxmf`, `crates/reticulum`, `crates/reticulum-daemon`).
- Public API drift is gated by `docs/contracts/baselines/lxmf-sdk-public-api.txt`.
- Contract behavior is governed by:
  - `docs/contracts/sdk-v2.md`
  - `docs/contracts/sdk-v2-events.md`
  - `docs/contracts/sdk-v2-errors.md`
