# LXMF-rs v0.5.2 Release Notes

Date: 2026-06-27
Release ref: `v0.5.2`

This is a patch release on the `0.5.x` line focused on improved Reticulum
interface management, with particular emphasis on LoRa/RNode operator
workflows needed by REM deployments.

The maintained parity source of truth remains `docs/status/current-roadmap.md`.

## Scope

- Improved LoRa/RNode status and management through `reticulumd` RPC,
  `rnstatus-rs`, and `rnodeconf-rs`.
- Broader Reticulum interface startup/status coverage for operators.
- Software smoke evidence for interface families that do not require hardware
  in CI.
- Continued crate and GitHub bundle release-train alignment for `0.5.2`.

## Highlights Since v0.5.1

- Added daemon/RPC RNode management dispatch for radio-state query, blink, safe
  read/display/local-radio operations, and guarded persistent/destructive
  management commands.
- Expanded `rnstatus-rs` LoRa/RNode summaries so operators can see probe state,
  radio state, configured endpoints, and runtime errors from the local daemon.
- Added `rnodeconf-rs` daemon-backed management coverage for serial/TCP RNode
  devices, feature-gated BLE RNode devices, and RNodeMulti parent/vport
  targets.
- Expanded RNodeMulti support with nested vport children, vport-aware
  management queueing, strict startup probing, ID-beacon fanout, and fake
  serial/TCP smoke evidence.
- Added or refreshed software smoke coverage for UDP loopback, Pipe subprocess,
  serial KISS/AX.25 KISS, KISS-over-TCP, Weave fake PTY, I2P fake SAM, and
  RNodeMulti fake PTY/TCP paths.
- Kept release packaging aligned with automated GitHub Actions publication:
  this PR prepares versions and release notes, while crates.io publication is
  performed by the release workflow when `v0.5.2` is created.

## Current Version Train

GitHub release version: `v0.5.2`

Public package versions are aligned to `0.5.2` for the automated release
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
- `rnodeconf-rs`
- `weaveconf-rs`
- `rnx`

## Validation Record

- PR #380 CI passed at `77b087e1c36f8f6aaa581a4814b732b26b7b7905` for
  `quality`, `build-matrix (stable)`, `test-nextest-unit`, and `contracts`.
- Focused interface smoke scripts listed in the parity matrix were exercised as
  part of the interface-parity branch evidence.
- `cargo xtask release-check` passed on the release-prep branch with local Java
  tooling on `PATH` and compiler/OpenSSL override environment variables unset.
- Final release tagging should still run the GitHub release workflow and the
  runbook smoke gates from `docs/runbooks/release-runbook.md`.

## Known Limits

- Full Python Reticulum/LXMF surface parity is not achieved.
- Real hardware evidence remains required for broader BLE RNode management,
  RNodeMulti production validation, Weave hardware validation, and some
  prepared-host interface scenarios.
- External-client compatibility claims for REM, RCH, Sideband, MeshChatX,
  Columba, or other clients require the corresponding external-client interop
  gates for that deployment.
