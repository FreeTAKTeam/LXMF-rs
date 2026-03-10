# LXMF Parity Matrix

Last reassessed: 2026-03-10 (`cargo test -p rns-rpc --lib`, `cargo test -p rns-transport --lib`, `cargo test -p lxmf-core --lib`)

Status legend: `not-started` | `partial` | `done`

`done` means the active workspace implements the behavior directly. RPC-domain placeholders, SDK-only state machines, or migration-only crates under `crates/internal/` do not qualify as full parity.

## Module Map

| Python Module | Rust Module | Status | Notes |
| --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | partial | Core message and wire-format behavior is present, but the full Python module surface is not fully mirrored. |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | done | Active workspace supports LXMF message payloads, wire packing, storage packing, signatures, propagation packing, and paper packing. |
| `LXMF/LXMPeer.py` | `crates/libs/lxmf-sdk` + `crates/libs/rns-rpc` | partial | Peer presence records and SDK event mapping exist, but real peer sync/acceptance/queue behavior is not yet equivalent to the Python reference. |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc` + `crates/apps/reticulumd` | partial | There is a working router/daemon path, but propagation-node behavior, delivery-mode policy handling, and paper/command flows are not full reference parity. |
| `LXMF/Handlers.py` | `crates/apps/reticulumd` + `crates/libs/rns-rpc` | partial | Delivery callbacks and bridge flows exist, but propagation and router side effects are narrower than the Python implementation. |
| `LXMF/LXStamper.py` | active workspace split across `crates/libs/lxmf-core` payload fields and `crates/libs/rns-rpc` legacy RPC helpers | partial | Stamp bytes can be carried in messages, but active-workspace stamp generation/validation/ticket parity is not complete. |

## Required Method-Level Checklist

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
- PARITY_ITEM id=stamper.cancel_work status=not-started
- PARITY_ITEM id=ticket.validity_with_grace status=partial
- PARITY_ITEM id=ticket.renewal_window status=not-started
- PARITY_ITEM id=ticket.derived_stamp status=not-started
- PARITY_ITEM id=peer.serialize_roundtrip status=partial
- PARITY_ITEM id=peer.queue_accounting status=not-started
- PARITY_ITEM id=peer.acceptance_rate status=not-started
- PARITY_ITEM id=peer.peering_key status=not-started
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
- PARITY_ITEM id=interop.python_live_gate status=not-started

## Confirmed Gaps

- Delivery-mode parity is incomplete. `send_message_v2` accepts `direct`, `opportunistic`, `propagated`, and `paper`, but the live transport bridge does not honor those options yet.
- Propagation-node parity is incomplete. The active RPC propagation flow is mostly a local payload store and metadata layer, not full LXMF peer/node sync behavior.
- Peer parity is incomplete. Peer records and events exist, but the active workspace does not yet match Python `LXMPeer` behavior.
- Paper-command parity is incomplete. `lxmf-core` has real paper wire helpers, but the daemon SDK layer still uses a simplified `lxm://{destination}/{message_id}` envelope path.
- Stamps and tickets are incomplete. The active workspace carries stamp fields and has legacy RPC helpers, but not full reference-grade active stamp/ticket semantics.
- Release B/C SDK domains are broader than Python LXMF, but many are app-domain state machines rather than wire/protocol parity.

## Reassessment Summary

- `lxmf-core` is the strongest parity area and should be treated as mostly complete for base message encoding/decoding.
- `reticulumd` and `rns-rpc` expose a large SDK surface, but a meaningful share of that surface is currently local domain logic rather than Python LXMF parity.
- Full LXMF parity should not be claimed until delivery-mode handling, propagation-node behavior, paper ingest/encode semantics, and stamp/ticket behavior are brought into the active workspace.
