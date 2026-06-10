# LXMF-rs v0.3.0-rc1 Release Notes

Date: 2026-06-10
Base commit: `5c08f4d87becce96e48b4ae2a15058d16e236f9c`

This is a release candidate for the next LXMF-rs product and daemon bundle
release. It is intended for integration testing, operator validation, and
feedback before the final `v0.3.0` release.

## Scope

- Usable Rust implementation of core Reticulum and LXMF behavior with repeatable
  compatibility evidence against pinned Python Reticulum and LXMF references.
- GitHub binary bundle release for `lxmd` and `reticulumd`.
- Not a claim of complete drop-in Python replacement parity. The maintained
  parity source of truth remains `docs/status/current-roadmap.md`.

## Highlights Since v0.2.0

- Expanded propagation peer and router behavior, including restored queue replay,
  unpeer cleanup, duplicate receive accounting, peer cost restoration, admission
  gates, and payload-backed peer queue snapshots.
- Broader remote propagation sync, fetch, download, unpeer, and lifecycle
  accounting coverage.
- Stronger Python compatibility coverage for peer sync, paper URI ingest, unknown
  handle behavior, and pinned reference interop.
- Added and stabilized production interface work for AutoInterface, KISS, LoRa,
  RNode BLE, and VR-N76 KISS-over-BLE paths.
- Added SDK and wrapper improvements, including contract/schema updates, Kotlin
  wrapper conformance, easier SDK examples, and ZeroMQ pipeline hardening.
- Added embedded mini-node and embedded interface support surfaces.
- Expanded CI, scheduled mesh/soak/HIL workflows, release readiness docs, and
  release bundle automation.

## Current Version Train

GitHub release version: `v0.3.0-rc1`

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

- Latest `main` CI before release prep:
  https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/27271895183
- Latest Python reference interop before release prep:
  https://github.com/FreeTAKTeam/LXMF-rs/actions/runs/27231818697
- June 10 scheduled Nightly Mesh Simulation, Nightly Soak Chaos, and Nightly
  Embedded HIL runs were green on the pre-RC base.

Local RC gates for this release-prep branch:

- `cargo xtask ci`: passed with `LXMF_RS_BASH` pointing to Git Bash.
- `cargo xtask ci --stage sdk-soak-check`: passed.
- `cargo xtask ci --stage schema-client-check`: passed.
- `cargo xtask schema-client-generate --check`: passed.
- `cargo xtask publish-crates --wave all --dry-run --allow-dirty`: passed
  through the release-tool fallback for unpublished local publish-wave
  dependencies.
- `cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20`: passed.
- `cargo fmt --all -- --check`: passed.
- `cargo xtask release-check`: attempted after fixing the failing sub-gates, but
  the local Windows run exceeded the one-hour command timeout during the soak
  portion. The soak gate passed when rerun directly.

## Known Limits

- Propagation and operational substitutability are still marked partial in
  `docs/status/current-roadmap.md`.
- Full Python surface parity is not achieved.
- External-client compatibility claims for Sideband, MeshChatX, and Columba
  require separate external-client interop gate evidence.
- Hardware and prepared-host evidence for all interface paths remains broader
  than the automated CI evidence.
