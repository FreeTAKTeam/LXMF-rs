# LXMF-rs v0.8.0 Release Notes

Date: 2026-07-10
Release ref: `v0.8.0`

This feature release consolidates the large Reticulum and LXMF compatibility
train landed since `v0.7.1`. It introduces the in-process LXMF runtime,
substantially expands typed SDK and daemon RPC behavior, strengthens direct and
propagated delivery, and closes additional software-testable Reticulum parity
gaps. All public packages and GitHub bundles move together to `0.8.0`.

Public API comparison against the published `0.7.1` crates found additive
changes only: no existing public items were removed or changed. The `0.8.0`
version marks the breadth of the new feature train rather than an intentional
source-breaking API transition.

## Highlights

- Adds the `lxmf-runtime` crate with an in-process backend over the existing
  Reticulum transport, including direct delivery, link reuse, runtime state,
  and typed capability negotiation.
- Expands `lxmf-sdk` with delivery cancellation, delivery tickets and stamp
  policy, propagation policy controls, typed propagation status, conversation
  listing, richer lifecycle metadata, and corresponding ZeroMQ pipeline
  methods.
- Strengthens direct and opportunistic delivery with reusable backchannels,
  receipt and delivery-trace metadata, duplicate and malformed inbound-drop
  events, resource limits, and destination mismatch reporting.
- Expands propagation behavior with control ACLs, convenience policy mutators,
  selected-node wakeups, processed-marker expiry, status accounting, and
  Python-compatible peer lifecycle handling.
- Adds Reticulum daemon RPC coverage for targeted announces, path metadata,
  link counts, blackholed identities, shared-instance status, path restore
  accounting, and runtime interface mutation.
- Persists blackholed identities across restarts and evicts cached paths tied
  to newly blackholed identities.
- Broadens hot-applied TCP server and UDP interface support to host-bound and
  device-bound configurations, including Python-style IPv4 broadcast defaults
  and device-selected IPv4/IPv6 TCP listeners.
- Completes the pinned Python `Resolver.py` surface with an executable
  reference probe while retaining Rust's additional cached identity resolver.
- Improves mesh discovery, Reticulum transport configuration, path-table
  persistence, AutoInterface behavior, Windows compatibility, ARM64 release
  bundles, and Columba integration evidence.

## Public Release Train

The following public packages are aligned at `0.8.0`:

- `lxmf`
- `lxmf-reference`
- `lxmf-wire`
- `lxmf-sdk`
- `lxmf-runtime`
- `reticulum-rs`
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

## Validation

- Public API diffs against published `0.7.1` found no removed or changed items
  in `lxmf-wire`, `lxmf-sdk`, `reticulum-rs-core`,
  `reticulum-rs-transport`, or `reticulum-rs-rpc`.
- Workspace formatting and all-feature clippy pass with warnings denied.
- Full `reticulum-rs-rpc`, `reticulum-rs-transport`, and `reticulumd` test
  suites pass.
- Architecture, module-size, and dependency-boundary gates pass.
- Local TCP and Unix shared-instance, pinned-Python shared-instance, software
  BLE management, and fake-TCP RNode prepared-host smokes pass.

## Known Limits

- Full Python Reticulum/LXMF surface parity is not yet claimed; consult the
  maintained parity matrices for the six remaining partial Reticulum rows.
- Prepared-host RNode serial, TCP/Wi-Fi, and BLE reports are still required for
  the strict interface parity audit.
- Broader RNodeMulti, Weave, VR-N76, public I2P/network soak, and external
  client evidence remains environment-dependent.
- Python utility and CRNS command ecosystems are not yet reproduced in full.
