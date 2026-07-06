# LXMF-rs v0.7.0 Release Notes

Date: 2026-07-06
Release ref: `v0.7.0`

This is the SDK-first release-train preparation for existing LXMF-rs SDK
communication. It promotes the current typed SDK path, ZeroMQ/RPC package
alignment, and operator-facing release metadata without claiming a separate
compatibility layer or complete drop-in Python Reticulum/LXMF parity.

The maintained parity source of truth remains `docs/status/current-roadmap.md`,
with row-level detail in `docs/status/reticulum-parity-matrix.md` and
`docs/status/lxmf-parity-matrix.md`.

## Scope

- Improved release alignment for the existing typed SDK communication path used
  by SDK-first REM/RCH integration planning.
- Continued public crate and GitHub bundle release-train alignment for `0.7.0`.
- Preserved explicit boundary that this release does not introduce or claim a
  compatibility layer for external clients.
- Retained the v0.6.x software parity baseline while keeping remaining parity
  gaps tied to the maintained roadmap and parity matrices.

## Highlights Since v0.6.0

- Prepared the public crate/package version train for `0.7.0` across the
  workspace SDK, Reticulum, LXMF, embedded, daemon, and tools packages.
- Improved the existing `ZmqPipelineBackendClient` send path in place:
  `sdk_send_v2` and `sdk_send_batch_v2` now preserve SDK request metadata for
  idempotency keys, TTL, correlation IDs, and extensions instead of dropping
  those fields before handoff.
- Added typed SDK app-surface helpers for direct-chat history and conversation
  lists, plus typed event helpers for inbound messages, inbound drops, receipt
  events, and delivery lifecycle states.
- Improved LoRa/RNode runtime status for software-managed serial/TCP paths by
  reporting safe management commands, queue depth, accepted/failed operation
  counters, the last management operation, and the last management error.
- Kept the existing SDK communication route as the integration thread rather
  than adding a sidecar compatibility surface.
- Refreshed release-train documentation and consumer examples so SDK users can
  target the `0.7.0` packages consistently.
- Preserved external dependency pins during the release metadata bump, including
  the existing `zeromq` dependency version.

## Current Version Train

GitHub release version: `v0.7.0`

Public package versions are aligned to `0.7.0` for the automated release
workflow:

- `lxmf`
- `reticulum-rs`
- `lxmf-reference`
- `lxmf-wire`
- `lxmf-sdk`
- `reticulum-rs-core`
- `reticulum-rs-transport`
- `reticulum-rs-rpc`
- `lxmf-embedded-mini`
- `rns-embedded-core`
- `rns-embedded-runtime`
- `rns-embedded-ffi`
- `rns-embedded-mininode`
- `lxmf-cli`
- `reticulumd`
- `rns-tools`

## Included GitHub Bundle Tools

- `lxmd`
- `lxmf`
- `lxmf-cli`
- `reticulumd`
- `lxm-interchange`
- `rnsd`
- `rnstatus-rs`
- `rnpath-rs`
- `rnodeconf-rs`
- `weaveconf-rs`
- `rnx`

## Validation Record

- Release metadata preparation updated the workspace `VERSION`, public package
  versions, path dependency versions, release notes, and current README release
  references for `v0.7.0`.
- Focused release-version metadata checks confirm public package versions are
  aligned to `0.7.0` while external dependencies such as `zeromq = "0.6.0"`
  remain unchanged.
- Focused SDK validation covers the native ZMQ send and batch-send metadata
  preservation, receipt terminality, typed inbound/drop/receipt events, native
  app-surface history and conversation helpers, and the full `lxmf-sdk` test
  suite with the `zmq-pipeline-backend` feature enabled.
- Focused daemon/RPC validation covers SDK-pollable direct inbound events,
  pre-handoff cancellation, restart persistence, LXMF bridge behavior, receipt
  bridge/worker behavior, Python compatibility dispatchability, and the issue
  #369 silent-error scanner.
- Focused LoRa/RNode software validation covers runtime status and safe
  management queue observability for ordinary serial/TCP LoRa paths, plus the
  feature-gated BLE RNode software tests where available.

## Known Limits

- Full Python Reticulum/LXMF surface parity is not achieved.
- This release does not add or claim a compatibility layer for REM, RCH,
  Sideband, MeshChatX, Columba, or other clients.
- External-client compatibility claims require the corresponding external-client
  interop gates for that deployment.
- Hardware/HIL evidence remains required for broader RNode, RNodeMulti, Weave,
  VR-N76, BLE, serial-radio, and prepared-host interface claims.
