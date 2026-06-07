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
| `LXMF/LXMPeer.py` | `crates/libs/rns-rpc`, `crates/apps/reticulumd` | partial | Persistent peers, queue marks, offer selection, policy gates, peering keys, throttling, maintenance, source accounting, cumulative acceptance, and boolean/list offer responses. | Complete transfer/retry/restart lifecycle and broad live peer interop remain. |
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
  IDs, preserve no-transfer liveness, and retain cumulative acceptance rates.
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
