# LXMF-rs v0.11.0

v0.11.0 is the RNode transport repair release. It keeps the LXMF and
Reticulum wire formats stable while making BLE/RNode receive ownership,
notification buffering, and firmware queue admission deterministic.

The repair was implemented in [PR #588](https://github.com/FreeTAKTeam/LXMF-rs/pull/588),
merged to `main` as `56ea9f06c474426b2245739e9cb5e2c325cdb1e2`, and consumed by
REM 1.4.

## Highlights

- A backend-owned bounded native read is awaited to completion before another
  read starts. A shorter cancellable outer timeout can no longer abandon a
  worker that consumes a later BLE notification.
- KISS data is buffered across normal BLE notification boundaries. A complete
  frame is not required to fit inside one ATT notification or a 173-byte MTU.
- RNode firmware queue admission follows the official `CMD_READY` query
  contract: one queued payload is admitted per ready response, with bounded
  polling while work remains.
- Resource transfer cleanup failures remain visible alongside the original
  transfer error.
- RNode traffic counters and startup status are exposed for diagnostics.

## Compatibility and migration

This is a minor release in the pre-1.0 version line because the public
`RnodeBleCommandMonitor::accept_degraded_startup` escape hatch was removed from
`reticulum-rs-transport`. Callers must use normal startup validation and wait
for `startup_validated()` before sending payloads. The runtime still accepts a
missing startup radio-state response only through its validated compatibility
path; malformed or mismatched responses remain fatal.

No LXMF wire-format or Reticulum packet-format change is intended. All 17
public workspace crates move to `0.11.0` together so published dependency
metadata remains coherent.

## Validation and publication

The release candidate must pass the strict workspace format, lint, test,
architecture, boundary, and release gates, followed by the exact-tag Release,
Verify, independent interoperability, performance, provenance, OCI, and
crates.io workflows. The final release evidence is recorded in the
`v0.11.0` release ledger after those workflows and public registry artifacts
are independently verified.
