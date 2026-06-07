# Current Roadmap Status

Last reassessed: 2026-06-07

This file is the repository-level source of truth for parity posture, release
confidence, and execution order. Detailed row-level status lives in:

- `docs/status/reticulum-parity-matrix.md`
- `docs/status/lxmf-parity-matrix.md`

Historical plans and issue lists explain how work was approached; they do not
override these status files.

## Current Position

LXMF-rs is a usable Rust implementation of Reticulum and LXMF with strong core
protocol coverage and repeatable interoperability against pinned Python
references. It is not yet a complete drop-in replacement for every Python
Reticulum/LXMF runtime, interface, router, and utility behavior.

The project is best described by capability level:

| Capability | Status | Meaning |
| --- | --- | --- |
| Wire compatible | achieved | Core Reticulum packet/identity primitives and LXMF message encodings are implemented and tested. |
| Direct-message interoperable | achieved | Selected bidirectional Rust/Python direct, link, channel, paper, and daemon paths are exercised in CI. |
| Propagation interoperable | partial | Propagated delivery and substantial peer/node behavior exist, but the complete Python router and peer lifecycle is not yet reproduced. |
| Operationally substitutable | partial | `reticulumd` is deployable and supports several production interfaces, but runtime, interface, and utility breadth remains narrower than Python. |
| Full Python surface parity | not achieved | Remaining gaps are tracked in the two parity matrices. |

## Strong Areas

### Reticulum

- Identity, destination, packet, cryptography, link, resource, and buffer
  behavior are the strongest RNS areas.
- Link establishment, proof validation, interface binding, watchdog timing,
  teardown, receipts, and resource lifecycle have active regression coverage.
- `reticulumd` supports TCP client/server, UDP, serial, KISS, AutoInterface,
  LoRa/RNode, feature-gated RNode BLE, and feature-gated VR-N76 KISS-over-BLE.
- AutoInterface has a live daemon runtime, including discovery, peer lifecycle,
  peer-data sockets, transport ingress, outbound routing, and multicast proof
  fallback.

### LXMF

- Message wire/storage packing, signatures, propagation packing, paper
  encoding, timestamp precision metadata, binary-field preservation, and
  Python-compatible storage containers are implemented.
- Delivery modes are honored by the daemon; the old claim that requested modes
  are ignored is obsolete.
- Direct and propagated resource sends support receipt-state separation,
  timeout/failure propagation, and active resource cancellation.
- Ticket validity, renewal, derivation, persistence, and inbound ticket reuse
  are implemented.
- Propagation peers have real queue, policy, maintenance, throttling, peering,
  offer-response, source-accounting, and acceptance-rate behavior. These are
  substantial implementations, not SDK-only placeholders.
- Local and remote peer-sync offer-response cleanup now preserves peers and
  propagation queues for retry on retryable or otherwise unexpected numeric
  responses, while still treating access denial and throttling as distinct
  Python paths.
- Retryable numeric local offer responses now mirror payload-backed live queue
  marks into the active peer record snapshot before returning, so restart/export
  state preserves the retry queue even when the serialized snapshot was empty.
- Restored Python peer records now update their serialized queue ID snapshot
  when peer sync handles, transfers, or transfer-limits queued offers, reducing
  restart/export drift after live offer-response processing.
- Peer sync queue creation also records newly queued existing propagation IDs in
  the peer record snapshot, so postponed syncs can restart/export with the same
  unhandled queue visible in live status.
- Inbound and remotely imported propagation payloads update active peer record
  snapshots when they queue new unhandled IDs or mark source peers handled,
  keeping restart/export state aligned with live queue fan-out and source
  accounting.
- Propagation purge cleanup removes deleted local payload IDs from active peer
  record snapshots, so restart/export state does not retain purged queue entries
  after the live peer marks have been cleared.
- Duplicate or replayed propagation queue attempts respect already-completed
  peer marks when updating peer record snapshots, avoiding restart/export drift
  that would reopen handled IDs as unhandled.
- Peer sync queue replay records preexisting live unhandled marks into the peer
  record snapshot even when the store did not insert new rows, preserving
  restart/export visibility for already-queued work.
- Reactivating a persisted `unpeered` record clears stale serialized peer queue
  snapshots before the peer becomes active again, avoiding restart/export
  resurrection of pre-unpeer propagation work.
- Peer sync stale queue cleanup now removes matching unhandled and completed
  IDs from active peer record snapshots when the underlying propagation payload
  no longer exists, keeping export/restart state aligned with live queue
  cleanup.
- Restored peer records now accept Python MessagePack binary
  `destination_hash`, handled, and unhandled IDs, prune serialized queue IDs
  whose payloads are missing during replay, and canonicalize/deduplicate the
  surviving IDs, avoiding restart/export drift when Python snapshot entries
  outlive or duplicate local propagation storage.
- Early transfer-limit decisions made before peering-key handling now update
  active peer record snapshots as completed work, keeping serialized state in
  sync with the live transfer-limited mark.

## Remaining Release Blockers

These are blockers to a broad "Python replacement" claim, not blockers to using
the implemented subset.

1. **Propagation router lifecycle**
   - Complete peer offer, transfer, retry, fetch, download, and synchronization
     behavior across success, denial, timeout, identity, and restart paths.
   - Close remaining `LXMRouter` side effects and persistent queue semantics.
2. **Deferred stamp lifecycle**
   - Add Python-style background work ownership, queueing, retry, cancellation,
     and progress behavior for expensive normal and propagation stamps.
3. **Interop breadth**
   - Add bidirectional live Python cases for every claimed delivery mode and
     newly completed peer/router row.
   - Capture release evidence for Sideband, MeshChatX, and Columba before making
     client-specific compatibility claims.
4. **Reticulum behavioral breadth**
   - Finish channel ordering, resolver/bootstrap, announce/path edge behavior,
     and runtime mutation parity.
5. **Operational breadth**
   - Add prepared-host hardware evidence for BLE/RNode paths.
   - Implement or explicitly defer missing Python interface families and
     utility commands.

## Active Execution Order

1. Finish propagation peer/router state machines.
2. Finish deferred stamp worker and retry lifecycle.
3. Expand pinned Rust/Python interoperability gates with each completed row.
4. Close RNS channel, discovery, resolver, and transport-policy gaps.
5. Collect hardware, soak, and external-client release evidence.
6. Expand interface and utility breadth after protocol behavior stabilizes.

## Verification Baseline

- Primary CI: `.github/workflows/ci.yml`
- Pinned Python interop: `.github/workflows/python-interop.yml`
- Reference revisions are declared in the interop workflow rather than copied
  into status prose.
- Current run status belongs in GitHub Actions, not in this maintained document.
- A passing Python-reference workflow proves only the scenarios it executes.

## Status Rules

- `done` requires active implementation plus active automated evidence.
- A local model, RPC projection, or SDK state machine alone does not establish
  Python protocol/runtime parity.
- A passing interop workflow does not promote unrelated matrix rows.
- Update this file and the affected matrix in the same change.
- Keep implementation history in Git and historical plans, not in this file.
