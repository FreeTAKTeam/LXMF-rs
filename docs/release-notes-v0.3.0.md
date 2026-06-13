# LXMF-rs v0.3.0 Release Notes

Date: 2026-06-12
Release commit: `53bca86cc59aef4e114c860ba5b766637f863817`

This is a usable sub-1.0 LXMF-rs product and daemon bundle release. It promotes
the `v0.3.0-rc1` line after additional propagation reliability, interop, and
operator-observability fixes landed on `main`.

This release is not a claim of complete drop-in Python Reticulum/LXMF
replacement parity. The maintained parity source of truth remains
`docs/status/current-roadmap.md`.

## Scope

- Usable Rust implementation of core Reticulum and LXMF behavior with repeatable
  compatibility evidence against pinned Python Reticulum and LXMF references.
- GitHub binary bundle release for `lxmd` and `reticulumd`.
- Focused propagation-node, peer-sync, remote transfer, retained payload, and
  operator-status improvements over `v0.2.0` and `v0.3.0-rc1`.
- Known partial parity for broader Python router lifecycle, external clients,
  and hardware/interface breadth.

## Highlights Since v0.2.0

- Expanded propagation peer and router behavior, including restored queue
  replay, unpeer cleanup, duplicate receive accounting, peer cost restoration,
  admission gates, throttling, transfer limits, retained haves, and
  payload-backed peer queue snapshots.
- Broader remote propagation sync, fetch, download, unpeer, lifecycle,
  failure-kind, cancellation, close-signal, and source-peer backoff handling.
- Stronger Python compatibility coverage for peer sync, paper URI ingest,
  unknown handle behavior, remote status/control, Python-origin `/offer`, and
  propagation `/get` haves acknowledgement.
- Added and stabilized production interface work for AutoInterface, KISS, LoRa,
  RNode BLE, and VR-N76 KISS-over-BLE paths.
- Added `rnstatus-rs` daemon status coverage, including interface runtime state
  and propagation peer state.
- Added SDK and wrapper improvements, including contract/schema updates, Kotlin
  wrapper conformance, easier SDK examples, and ZeroMQ pipeline hardening.
- Added embedded mini-node and embedded interface support surfaces.
- Expanded CI, scheduled mesh/soak/HIL workflows, release readiness docs, and
  release bundle automation.

## Changes Since v0.3.0-rc1

- Added live propagation interop cases for Python-origin `/offer` and `/get`
  haves acknowledgement.
- Preserved propagation lifecycle and queue state across no-access, ignored,
  malformed, failed, cancelled, and terminal remote transfer paths.
- Added source-peer backoff and recovery handling for failed or malformed remote
  fetch/download/import paths.
- Improved peer-sync visibility and retry behavior, including structured failure
  kinds, hidden-peer control lookup, failed local offer state, and payload-backed
  queue snapshot preservation.
- Improved retained propagation haves filtering, storage-pruning priority, and
  propagation ingest rejection semantics.
- Surfaced remote resource terminal events and remote link close signals for
  operator-visible diagnostics.
- Added BLE MTU readiness diagnostics.
- Added the Codex PR review workflow.

## Current Version Train

GitHub release version: `v0.3.0`

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

- Latest `main` CI before final release prep:
  https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/27434941083
- Latest Python reference interop before final release prep:
  https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/27431799116
- `v0.3.0-rc1` published Linux, macOS, and Windows daemon bundles with matching
  `.sha256` files.

Local RC gates for the release candidate branch included:

- `cargo xtask ci`: passed with `LXMF_RS_BASH` pointing to Git Bash.
- `cargo xtask ci --stage sdk-soak-check`: passed.
- `cargo xtask ci --stage schema-client-check`: passed.
- `cargo xtask schema-client-generate --check`: passed.
- `cargo xtask publish-crates --wave all --dry-run --allow-dirty`: passed
  through the release-tool fallback for unpublished local publish-wave
  dependencies.
- `cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20`: passed.
- `cargo fmt --all -- --check`: passed.

## Known Limits

- Propagation interoperability and operational substitutability are still marked
  partial in `docs/status/current-roadmap.md`.
- Full Python surface parity is not achieved.
- External-client compatibility claims for Sideband, MeshChatX, and Columba
  require separate external-client interop gate evidence.
- Hardware and prepared-host evidence for all interface paths remains broader
  than the automated CI evidence.
