# LXMF-rs v0.5.0 Release Notes

Date: 2026-06-19
Release ref: `v0.5.0`

This is the LXMF/RNS communication parity milestone release. It promotes the
daemon, SDK, propagation, peer lifecycle, stamp lifecycle, and RNS Channel work
needed by supported LXMF clients and service integrations while keeping the
project's broader Python-replacement limits explicit.

This release is not a claim of complete drop-in Python Reticulum/LXMF
replacement parity. The maintained parity source of truth remains
`docs/status/current-roadmap.md`.

## Scope

- Python-reference propagation router lifecycle parity for remote status,
  fetch, download, sync, failures, and restart-visible side effects.
- Python-only `LXMPeer.py` lifecycle parity for persistent queues, offer
  responses, transfer/retry behavior, source accounting, and unpeer cleanup.
- Deferred normal and propagation stamp worker lifecycle parity, including
  queue ownership, retry metadata, cancellation, and progress visibility.
- RNS Channel ordered receive and callback parity, including contiguous
  delivery, duplicate/window rejection, handler ordering, panic containment,
  delivery-on-proof, retry timeout, and live Rust/Python sequence evidence.
- GitHub release bundles now include all implemented user-facing tool binaries:
  `lxmd`, `lxmf`, `lxmf-cli`, `reticulumd`, `lxm-interchange`, `rnsd`,
  `rnstatus-rs`, and `rnx`.

## Highlights Since v0.4.1

- Closed the propagation router lifecycle gate with live Python-reference
  remote lifecycle coverage.
- Closed the peer lifecycle parity gate across restart replay, offers,
  transfer limits, retry/failure classes, unpeer cleanup, haves, and source
  accounting.
- Added propagation-node configuration parity, outbound propagation cost lookup,
  and local storage for self-selected propagation nodes.
- Preserved phone/client-visible LXMF fields more accurately for Sideband and
  Columba-style payload decoding.
- Added deferred stamp lifecycle ownership and active delivery-pipeline progress
  reporting before message/resource handoff.
- Closed RNS Channel sequencing and callback behavior gaps against
  `RNS/Channel.py`.

## Current Version Train

GitHub release version: `v0.5.0`

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

- PR #365 closed deferred stamp lifecycle parity with normal CI, peer lifecycle
  essential CI, and pinned Python reference interop passing.
- PR #366 closed RNS Channel parity with normal CI passing and local ignored
  Rust/Python channel interop passing.
- The release commit was validated with the release runbook gates before tag
  publication.

## Known Limits

- Full Python surface parity is not achieved.
- Operational substitutability remains partial where Python has broader runtime
  mutation, resolver/bootstrap, interface-family, utility, and hardware-host
  coverage.
- External-client compatibility claims for Sideband, MeshChatX, Columba, or
  other third-party clients require separate external-client interop gate
  evidence.
