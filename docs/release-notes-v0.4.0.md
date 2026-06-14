# LXMF-rs v0.4.0 Release Notes

Date: 2026-06-14
Release ref: `v0.4.0`

This is the ZeroMQ SDK integration release for REM 1.1.1 and RCH pre-3.0
Rust consumers. It promotes the typed `lxmf-sdk` ZeroMQ path through
`ZmqPipelineBackendClient` so clients can rely on SDK methods instead of raw
RPC/HTTP calls for the release scope below.

This release is not a claim of complete drop-in Python Reticulum/LXMF
replacement parity. The maintained parity source of truth remains
`docs/status/current-roadmap.md`.

## Scope

- Typed ZeroMQ SDK foundation for REM/RCH client integration.
- Basic LXMF wire field preservation through the SDK send and batch-send paths.
- Peer discovery, saved-peer lifecycle, capability announce metadata, display
  names, callsigns, stale-peer filtering, and reconnect-oriented peer state.
- Durable direct-chat SDK flows for one-to-one messages, conversation summaries,
  receipt visibility, retry/cancel status metadata, restart-recovered history,
  link-bearing message bodies, and burst-send result stability.
- Propagation-first SDK coverage for peer sync, remote fetch/download/sync,
  unpeer, node selection, local propagation status/config, delivery policy,
  payload ingest/fetch, sync acknowledgement, recovery state, retry/timeout,
  denial, queue cleanup, and persistent queue visibility.

## Highlights Since v0.3.0

- Added typed ZeroMQ SDK peer identity, directory, stale-filter, saved-contact,
  peer lifecycle, and capability announce surfaces.
- Preserved documented LXMF custom field keys and `_lxmf_fields_msgpack_b64`
  through the typed SDK send path for REM/RCH payload compatibility.
- Treated payload `body` as direct-chat message content when `content` is
  absent while still preserving the original `body` field for client renderers.
- Added typed batch-send, cancel, status retry metadata, receipt terminality,
  local delivery-destination lookup, durable history, and conversation summary
  flows over the SDK envelope path.
- Added typed propagation SDK methods and result projections for peer sync,
  remote transfer lifecycle, propagation node lifecycle, local propagation
  lifecycle, payload store access, recovery state, policy metadata, transfer
  accounting, queue snapshots, retry/postponement, denial, timeout, and
  restart-recovery diagnostics.
- Updated roadmap and parity status to describe the ZeroMQ SDK release path
  while keeping application-level REM/RCH schemas out of scope.

## Current Version Train

GitHub release version: `v0.4.0`

Crate/package versions intentionally remain per the publish plan rather than one
blanket workspace version:

- `lxmf`: `0.3.0`
- `reticulum-rs-rpc`: `0.3.0`
- `lxmf-sdk`: `0.2.1`
- `lxmf-wire`: `0.2.0`
- `reticulum-rs-core`: `0.2.0`
- `reticulum-rs-transport`: `0.2.0`
- app/tool crates remain unpublished and are distributed through GitHub bundles

## Validation Record

- Main integration PR for the ZeroMQ SDK stack:
  https://github.com/FreeTAKTeam/LXMF-rs/pull/340
- PR #340 required checks passed: `quality`, `correctness`,
  `build-matrix (stable)`, `test-nextest-unit`, and `contracts`.
- Focused local checks before integration included:
  - `cargo test -p lxmf-sdk --features zmq-pipeline-backend zmq_pipeline -- --nocapture --test-threads=1`
  - `cargo test -p reticulum-rs-rpc sdk_propagation -- --nocapture`
  - `cargo fmt --all -- --check`
  - `cargo check -p lxmf-sdk --features zmq-pipeline-backend`
  - `cargo clippy -p lxmf-sdk -p reticulum-rs-rpc --lib --all-features --no-deps -- -D clippy::manual_assert -D clippy::redundant_clone -D clippy::iter_cloned_collect`
  - `C:\Program Files\Git\bin\bash.exe tools/scripts/check-module-size.sh`

## Known Limits

- Propagation interoperability and operational substitutability are still marked
  partial in `docs/status/current-roadmap.md`.
- Full Python surface parity is not achieved.
- Application-level REM/RCH schemas are out of scope for this release; LXMF-rs
  provides the basic LXMF fields and typed SDK transport behavior those clients
  need.
- External-client compatibility claims for Sideband, MeshChatX, and Columba
  require separate external-client interop gate evidence.
- Hardware and prepared-host evidence for all interface paths remains broader
  than the automated CI evidence.
