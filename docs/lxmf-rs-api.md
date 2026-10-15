# API Surface and Stability

This guide identifies application entry points and the scope of API checks.
Use the [package guide](project-layout.md) for the complete current inventory
and [SDK integration guide](sdk/README.md) for application setup.

## Application entry points

Prefer `lxmf_sdk::app::Client` with typed `runtime()`, `messages()`, `events()`,
`identity()`, and `attachments()` handles. The typed ZeroMQ backend is the
preferred desktop/service path; Unix and HTTP/TLS RPC remain supported.
`lxmf-runtime` is the active in-process backend for hosts embedding transport
execution. Lower-level `LxmfSdk` methods remain available for explicit cursor
recovery, diagnostics, and manual embedded hosts.

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
- `crates/libs/lxmf-runtime`
  - in-process backend implementing the SDK over the Reticulum runtime
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
- `crates/libs/lxmf-embedded-mini` and `crates/libs/rns-embedded-mininode`
  - constrained-device and minimal-node integration; capabilities remain
    crate-specific

`Cargo.toml` is the source of truth for active workspace members.
`lxmf-reference` holds pinned compatibility metadata. `test-support` and
`xtask` are unpublished repository tooling, not public application APIs.
The patched `reticulum-rs-zeromq` dependency is published separately from
`vendor/zeromq` with its own retained upstream license.

## Operator/App Surfaces

- `crates/apps/lxmf-cli`: operator-facing CLI over `lxmf-sdk`
- `crates/apps/reticulumd`: daemon binary hosting `reticulum-rs-rpc`
- `crates/apps/rns-tools`: diagnostics and interop helpers

App crates are not intended as stable library APIs.

## Stability Policy

- No legacy crate path compatibility guarantees (`crates/lxmf`, `crates/reticulum`, `crates/reticulum-daemon`).
- SDK public API drift is gated by `docs/contracts/baselines/lxmf-sdk-public-api.txt`;
  that SDK baseline does not certify every workspace crate's public API.
- Contract behavior is governed by:
  - `docs/contracts/sdk-v2.md`
  - `docs/contracts/sdk-v2-events.md`
  - `docs/contracts/sdk-v2-errors.md`

Related references:

- `docs/contracts/sdk-v2-api-stability.md`
- `docs/contracts/support-policy.md`
- `README.md`

Published crates.io entry points and crate responsibilities are listed in the
[workspace and package guide](project-layout.md). Core SDK contract v2.6
retains the `v2` schema namespace and protocol version `2`; these are separate
from the package release number. Capability negotiation and documented
pre-1.0 migration rules govern compatibility.

Use the root `Cargo.toml`, crates.io badges in `README.md`, or the selected
release tag for version numbers. This page intentionally does not duplicate
mutable release versions.
