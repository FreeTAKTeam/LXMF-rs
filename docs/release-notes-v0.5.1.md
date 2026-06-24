# LXMF-rs v0.5.1 Release Notes

Date: 2026-06-20
Release ref: `v0.5.1`

This is a patch release on the `0.5.x` line focused on Sideband/RCH
interoperability, propagation delivery reliability, resource-transfer
compatibility, and diagnostics after the `v0.5.0` parity milestone.

The maintained parity source of truth remains `docs/status/current-roadmap.md`.

## Scope

- Sideband propagation and resource-transfer compatibility fixes.
- Propagation delivery behavior for stale or previously announced peers.
- Reticulumd diagnostics for previously silent issue #369 failure paths.
- Release bundle continuity for all implemented user-facing tool binaries.

## Highlights Since v0.5.0

- Fixed propagation stamp round encoding.
- Improved issue #369 diagnostics and logging for inbound resource, ratchet,
  ZMQ/RPC ingress, and JSON-RPC error paths.
- Fixed Sideband propagation resource request handling.
- Accepted Sideband resource part-count shapes seen in live phone HIL.
- Persisted announce identities so propagated delivery can still resolve stale
  peer delivery identities.
- Selected the local propagation node on startup when configured, preserving the
  propagation-node behavior expected by RCH/Sideband test deployments.

## Current Version Train

GitHub release version: `v0.5.1`

Crate/package versions intentionally remain per the publish plan rather than one
blanket workspace version:

- `lxmf`: `0.3.0`
- `reticulum-rs-rpc`: `0.3.0`
- `lxmf-sdk`: `0.2.1`
- `lxmf-wire`: `0.2.0`
- `reticulum-rs-core`: `0.2.0`
- `reticulum-rs-transport`: `0.2.0`
- app/tool crates remain unpublished and are distributed through GitHub bundles

## Included GitHub Bundle Tools

- `lxmd`
- `lxmf`
- `lxmf-cli`
- `reticulumd`
- `lxm-interchange`
- `rnsd`
- `rnstatus-rs`
- `rnx`

## Validation Record

- Main CI passed for the merged post-`v0.5.0` fixes through PR #376.
- Focused local storage tests passed while merging
  `codex/persist-announce-identities` into `main`.
- Release bundle publication is handled by `.github/workflows/release-bundles.yml`
  on the `v0.5.1` tag.

## Known Limits

- Full Python Reticulum/LXMF surface parity is not achieved.
- Operational substitutability remains partial where Python has broader runtime
  mutation, resolver/bootstrap, interface-family, utility, and hardware-host
  coverage.
- External-client compatibility claims require separate external-client interop
  gate evidence.
