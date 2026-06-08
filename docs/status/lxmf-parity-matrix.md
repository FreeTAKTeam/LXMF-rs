# LXMF Parity Matrix

Last reassessed: 2026-06-07

This is the maintained row-level status for Python LXMF compatibility.
Repository-level posture and execution order live in
`docs/status/current-roadmap.md`.

Status legend:

- `done`: implemented in the active workspace and backed by active tests.
- `partial`: useful behavior exists, but identified Python behavior or evidence
  remains missing.
- `not-started`: no meaningful active implementation.

Workspace paths are used for navigation. `crates/libs/lxmf-core` publishes as
`lxmf-wire`; `crates/libs/rns-rpc` publishes as `reticulum-rs-rpc`.

## Module Matrix

| Python module | Rust surface | Status | Implemented baseline | Residual gap |
| --- | --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | partial | Constants, payload fields, message identity, inbound decoding, and wire helpers. | The complete convenience/module surface is not mirrored. |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | done | Wire, storage, propagation, paper, signatures, message IDs, binary fidelity, and timestamp precision metadata. | No confirmed base-message blocker. |
| `LXMF/LXMPeer.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Persistent peers, queue marks, offer selection, policy gates, peering keys, throttling, maintenance, source accounting, cumulative acceptance, serialized restored queue snapshots, and boolean/list/numeric offer responses. | Complete transfer/retry/restart lifecycle and broad live peer interop remain. |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Outbound modes, selected propagation nodes, direct/propagated resources, cancellation, fetch/download/sync RPCs, receipts, persistence, and status. | Full Python queue, retry, propagation-node, and command side effects remain. |
| `LXMF/Handlers.py` | `crates/apps/reticulumd`, `crates/libs/rns-rpc` | partial | Delivery, announce, propagation app-data, receipt, and inbound bridge handling. | Some router-coupled side effects and negative/drop observability remain narrower. |
| `LXMF/LXStamper.py` | `crates/libs/lxmf-core`, `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Validation, generation, ticket-derived stamps, cancellation-aware task work, and lifecycle metadata. | Python-style deferred worker queue, retry ownership, and continuous progress remain. |

## Method Checklist

- PARITY_ITEM id=message.pack_wire status=done
- PARITY_ITEM id=message.unpack_wire status=done
- PARITY_ITEM id=message.storage_roundtrip status=done
- PARITY_ITEM id=message.propagation_pack_unpack status=done
- PARITY_ITEM id=message.paper_pack status=done
- PARITY_ITEM id=message.paper_uri_helpers status=done
- PARITY_ITEM id=message.file_unpack_helpers status=done
- PARITY_ITEM id=message.signature_verify status=done
- PARITY_ITEM id=message.object_accessors status=done
- PARITY_ITEM id=stamper.validate_pn_stamp status=partial
- PARITY_ITEM id=stamper.generate_stamp status=partial
- PARITY_ITEM id=stamper.cancel_work status=partial
- PARITY_ITEM id=stamper.outbound_progress_queries status=partial
- PARITY_ITEM id=ticket.validity_with_grace status=done
- PARITY_ITEM id=ticket.renewal_window status=done
- PARITY_ITEM id=ticket.derived_stamp status=done
- PARITY_ITEM id=peer.serialize_roundtrip status=partial
- PARITY_ITEM id=peer.queue_accounting status=partial
- PARITY_ITEM id=peer.acceptance_rate status=partial
- PARITY_ITEM id=peer.peering_key status=partial
- PARITY_ITEM id=router.outbound_queue status=partial
- PARITY_ITEM id=router.handle_outbound_policy status=partial
- PARITY_ITEM id=router.adapter_transport status=partial
- PARITY_ITEM id=router.paper_uri_ingest status=partial
- PARITY_ITEM id=router.cancel_outbound status=partial
- PARITY_ITEM id=router.propagation_ingest_fetch status=partial
- PARITY_ITEM id=router.transfer_state_lifecycle status=partial
- PARITY_ITEM id=router.node_app_data status=partial
- PARITY_ITEM id=handlers.delivery_callback status=partial
- PARITY_ITEM id=handlers.propagation_app_data status=partial
- PARITY_ITEM id=handlers.router_side_effects status=partial
- PARITY_ITEM id=interop.python_live_gate status=done

## Capability Detail

### Messages and interchange

- Python-compatible wire and storage containers are emitted and accepted.
- Propagation and paper packing use canonical `lxmf-wire` helpers.
- Signed messages, fields, attachment aliases, floating timestamps, and
  non-UTF8 title/content bytes retain client-visible fidelity.

### Delivery and receipts

- Direct, opportunistic, propagated, and paper modes are distinct.
- Transport completion remains `sent`; final delivery receipts produce
  `delivered`.
- Resource advertisement failure, retry exhaustion, timeout, and explicit
  cancellation reach daemon message state.

### Tickets and stamps

- Ticket grace, renewal, derivation, persistence, and reply reuse are complete.
- Inbound normal and propagation stamps honor configured flexibility.
- Outbound normal and propagation work records generating, ready, failed, and
  cancelled state.
- The remaining gap is background queue/worker/retry behavior, not basic stamp
  cryptography or ticket semantics.

### Peers and propagation

- Peer behavior includes static/discovered admission, peering cost/timebase,
  queue accounting, sync/transfer limits, stamp policy, throttling, candidate
  selection, unreachable culling, low-acceptance rotation, and prioritized
  offers.
- Offer responses support Python boolean and list forms, reject out-of-offer
  IDs, preserve no-transfer liveness, retain cumulative acceptance rates, and
  preserve peers and queues on retryable or otherwise unexpected numeric
  offer-response cleanup paths.
- Retryable numeric local offer responses mirror payload-backed live handled and
  unhandled queue marks into active peer record snapshots before returning,
  preserving restart/export retry state even when the serialized snapshot was
  previously empty.
- Retryable, throttled, generic failed, and malformed-import remote peer-sync
  paths mirror the same payload-backed queue marks into active peer record
  snapshots before publishing the failed sync event, so local and remote
  retry/export behavior stays aligned.
- Payload-backed remote failure snapshots replace stale serialized peer queue
  IDs with live payload-backed marks, so bridge failures do not preserve
  obsolete restart/export work after the underlying payload is gone.
- Zero-cost peer stamp policies transfer unstamped queued offers immediately
  without waiting for absent peering metadata, matching the Python no-stamp
  path and avoiding repeated peer-sync postponement.
- Malformed remote fetch and download imports mirror existing payload-backed
  queue marks into active peer record snapshots before returning the import
  failure, so already queued relay work remains visible after restart/export.
- Remote fetch and download bridge failures mirror existing payload-backed
  queue marks into active peer record snapshots before returning the failure,
  so already queued relay work remains visible after restart/export.
- Remote fetch and download bridge-unavailable errors mirror existing
  payload-backed queue marks into active peer record snapshots before
  returning, so queued relay work remains visible after restart/export even
  when no bridge is configured.
- Successful remote fetch and download mirror existing payload-backed queue
  marks into active peer record snapshots after applying imports, preserving
  queued retry work across restart/export even when the remote transfer succeeds
  without consuming those local queued offers.
- Remote peer-sync backoff postponements mirror existing payload-backed queue
  marks into active peer record snapshots before returning, so deferred syncs
  preserve queued retry work across restart/export.
- Remote peer-sync bridge-unavailable errors mirror existing payload-backed
  queue marks into active peer record snapshots for already known peers before
  returning, including case-insensitive requests, without creating new peers
  when the bridge is absent.
- Remote peer-sync bridge-unavailable errors for already known peers also
  publish the failed peer-sync event and mark the propagation sync lifecycle
  failed, keeping queued retry state observable without creating new peers.
- Successful remote peer-sync mirrors existing payload-backed live queue marks
  into active peer record snapshots after applying imports, preserving queued
  retry work across restart/export even when the remote sync succeeds without
  transferring those local queued offers.
- Remote peer-sync uses the stored peer ID case for the bridge call, import
  source accounting, state updates, and response envelope when callers supply a
  case-variant peer request.
- Failed remote unpeer attempts mirror existing payload-backed queue marks into
  active peer record snapshots before returning bridge-unavailable or
  bridge-execution errors, including case-insensitive peer requests, so failed
  peering teardown preserves queued retry work across restart/export.
- Successful remote unpeer uses the stored peer ID case for the bridge call and
  nested bridge result when callers supply a case-variant peer request, keeping
  remote teardown identity aligned with local queue cleanup.
- Payload-backed peer queue snapshot mirroring resolves stored peer IDs
  case-insensitively before reading live queue marks, preserving queued
  restart/export work when callers use Python-style peer case variants.
- Incremental peer queue snapshot updates resolve stored peer IDs before
  checking completed live marks, so transfer-limited or handled work is not
  serialized as retryable unhandled queue state through peer case variants.
- Incremental peer queue snapshot helpers canonicalize transient IDs before
  serializing handled or unhandled queue state, so padded or upper-case caller
  IDs do not leak into restart/export snapshots.
- Transfer-limited peer marks remain terminal when a later generic handled
  report arrives, so transfer-limit retry decisions are not reclassified as
  offered/handled work in peer queue accounting.
- Static-only propagation peer replacement routes removed static peers through
  the same local unpeer cleanup as explicit unpeer, so handled, received,
  transfer-limited, and unhandled queue marks are cleared and accounted
  consistently.
- Completed peer mark helpers write and read received/transferred live marks
  under the stored peer ID case when a peer record already exists, keeping live
  queue state and serialized restart/export snapshots aligned.
- Restored peer record queue IDs are replayed into the live store, newly queued
  existing and inbound/imported propagation IDs are reflected in the serialized
  peer snapshot, source-peer handled IDs are preserved for restart/export, and
  offer-response handling keeps IDs in sync when queued messages become handled,
  transferred, or transfer-limited.
- Remote import batches deduplicate accepted transient IDs before peer queue
  and incoming-message side effects are applied, so duplicate payloads in one
  fetch/download/sync response do not inflate peer queue accounting.
- Purging local propagation payloads removes matching deleted IDs from active
  peer record snapshots, preventing restart/export drift after queue cleanup.
- Duplicate or replayed propagation queue attempts preserve completed peer
  snapshot state instead of reopening handled IDs as serialized unhandled work.
- Peer sync queue replay mirrors preexisting live unhandled marks into active
  peer record snapshots, keeping restart/export state aligned even when no new
  store rows were inserted.
- Rejoining from a persisted `unpeered` peer record clears stale serialized
  queue snapshots before the peer is active again, preventing pre-unpeer work
  from being restored on export/restart.
- Peer sync stale queue cleanup prunes matching active peer record snapshot IDs
  for unhandled and completed marks when the propagation payload has already
  been removed, keeping serialized restart/export state aligned with live queue
  cleanup.
- Restored peer record replay accepts Python MessagePack binary
  `destination_hash`, handled, and unhandled IDs, prunes serialized IDs whose
  payloads are absent, and canonicalizes/deduplicates surviving IDs, so stale
  or repeated Python snapshot entries are not exported again after replay.
- Transfer-limit decisions made before peering-key handling update active peer
  record snapshots as completed queue work, so restart/export state reflects
  the live transfer-limited mark.
- Inbound propagation distinguishes clients, validated peers, unpeered
  identified senders, and local delivery; source peers are accounted and not
  re-offered their own payloads.
- These behaviors materially narrow the gap, but complete Python peer transfer,
  restart recovery, and router queue lifecycle remain unproven.

## Highest-Priority Gaps

1. Complete peer transfer, retry, restart, and persistent queue lifecycle.
2. Complete propagation-node fetch/download/sync and router side effects.
3. Add deferred stamp queue ownership and retry semantics.
4. Expand live bidirectional Python interop for propagation and peer rows.
5. Validate external clients before making client-specific claims.

## Evidence

- `.github/workflows/python-interop.yml` runs pinned Python reference
  conformance plus live channel, paper, compatibility-matrix, and LXMD
  remote-relay tests.
- Focused daemon/RPC tests cover delivery modes, propagation offers, peer
  maintenance, queue policy, source accounting, stamps, tickets, receipts, and
  cancellation.
- `interop.python_live_gate` means the configured scenarios run successfully;
  it does not imply every partial row is complete.
