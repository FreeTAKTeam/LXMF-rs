# LXMF Parity Matrix

Status: historical parity snapshot; check `docs/status/current-roadmap.md` for
current repo-wide status before relying on this file for active execution order.
As of 2026-06-02, the live Python-reference interop workflow is green for the
current branch, but that checkpoint does not convert the partial peer, router,
propagation, and stamper rows below into full parity.

Last reassessed: 2026-05-07 (`cargo run -p xtask -- ci`, `cargo run -p xtask -- architecture-checks`, live local Rust/Python interop gates)

Status legend: `not-started` | `partial` | `done`

`done` means the active workspace implements the behavior directly. RPC-domain placeholders, SDK-only state machines, or migration-only crates under `crates/internal/` do not qualify as full parity.

Naming note: this matrix keeps workspace paths such as `crates/libs/lxmf-core`
and `crates/libs/rns-rpc` for code-navigation clarity. The published package
names are `lxmf-wire` and `reticulum-rs-rpc`.

## Module Map

| Python Module | Rust Module | Status | Notes |
| --- | --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | partial | Core message and wire-format behavior is present, but the full Python module surface is not fully mirrored. |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | done | Active workspace supports LXMF message payloads, wire packing, storage packing, signatures, propagation packing, and paper packing. |
| `LXMF/LXMPeer.py` | `crates/libs/lxmf-sdk` + `crates/libs/rns-rpc` | partial | Peer presence records and SDK event mapping exist, but real peer sync/acceptance/queue behavior is not yet equivalent to the Python reference. |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc` + `crates/apps/reticulumd` | partial | There is a working router/daemon path, but propagation-node behavior and some command/peer side effects are not full reference parity. |
| `LXMF/Handlers.py` | `crates/apps/reticulumd` + `crates/libs/rns-rpc` | partial | Delivery callbacks and bridge flows exist, but propagation and router side effects are narrower than the Python implementation. |
| `LXMF/LXStamper.py` | active workspace split across `crates/libs/lxmf-core`, `crates/libs/rns-rpc`, and `crates/apps/reticulumd` | partial | Stamp bytes can be carried in messages, outbound stamp/ticket work is active, cancellation-aware generation is used by delivery tasks, and ticket reuse/renewal semantics are implemented. Full Python deferred-stamp queue/progress parity is still incomplete. |

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

## Confirmed Gaps

- Delivery-mode parity baseline is implemented in active `reticulumd` tests.
  Treat deeper propagation-router behavior as the remaining open area instead
  of the old "bridge ignores delivery mode" claim.
- Daemon status and peer scoring now preserve Python's `sent` vs `delivered`
  distinction for active send/resource completion and delivery-receipt paths.
- Tracked outbound resource timeout now propagates as a failed daemon receipt
  instead of silently dropping transport state.
- Propagation-node parity is incomplete. The active RPC propagation flow is mostly a local payload store and metadata layer, not full LXMF peer/node sync behavior.
- Peer parity is incomplete. Peer records, configured static peers, events,
  runtime counters, acceptance-rate/backoff fields, Python-style message
  accounting, per-peer propagation transfer/sync limits, propagation stamp
  policy, and peering-key status values are exposed, but the active workspace
  does not yet match Python `LXMPeer` queueing, transfer, and peering behavior.
- Paper-command baseline is implemented for bridge-backed `reticulumd`: SDK
  paper encode/decode uses canonical `lxmf-wire` paper URI helpers and tests
  reject the old placeholder `lxm://{destination}/{message_id}` path. Broader
  router command side effects remain partial.
- Stamps and tickets are still incomplete as a combined lifecycle area. The
  active workspace carries stamp fields, normal and propagation stamp
  generation have cancellation-aware delivery-task code paths, ticket
  issue/reuse follows the Python renewal window, renewed inbound tickets keep
  older unexpired tickets valid like Python, outbound ticket stamps are
  generated, signed inbound tickets are remembered for replies, and
  Python-style expired ticket cleanup is implemented. Python-style outbound
  progress and stamp-cost queries exist over stored message state. Inbound
  delivery-stamp validation and local propagated-message stamp metadata apply
  the Python-compatible configured flexibility floors. The
  remaining gap is the full Python deferred-stamp queue, live worker progress,
  and retry lifecycle.
- Outbound cancellation is stronger than a status-only marker: spawned
  `reticulumd` delivery tasks now observe persisted `cancelled` state at
  scheduling, payload, identity-wait, propagation, and link-send boundaries.
  Tracked resource-backed sends now monitor persisted cancel state and abort
  the active Reticulum resource with `ResourceInitiatorCancel` when the user
  cancels after the resource has started. It remains partial because the full
  Python router work queue and retry lifecycle are not yet mirrored.
- Release B/C SDK domains are broader than Python LXMF, but many are app-domain state machines rather than wire/protocol parity.

## Reassessment Summary

- `lxmf-wire` is the strongest parity area and should be treated as mostly complete for base message encoding/decoding.
- `reticulumd` and `reticulum-rs-rpc` expose a large SDK surface, but a meaningful share of that surface is currently local domain logic rather than Python LXMF parity.
- Full LXMF parity should not be claimed until propagation-node behavior,
  command/peer side effects, and remaining stamp worker lifecycle behavior are
  brought into the active workspace.

Recent focused evidence:

- `cargo test -p reticulum-rs-rpc --lib ticket_generate_renews_ticket_inside_renewal_window -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib ticket_renewal_keeps_old_unexpired_ticket_valid_like_python -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib ticket -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib ticket_generate_reuses_persisted_ticket_after_daemon_restart -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib outbound_lxm -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib propagation_remote_sync -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib propagation_acknowledge_sync_completion -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib list_peers_exposes_python_style_message_counters -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib propagation_enable_activates_static_peers_like_python -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib announce_received_parses_propagation_peer_name_from_python_metadata -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib new_peer_acceptance_rate_matches_python_zero_offer_default -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib list_peers_exposes_peering_key_value_when_cost_is_known -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib autopeered_announce_records_propagation_peer_state -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd python_status_exposes_peer_peering_key_value -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd python_status_prefers_peer_propagation_stamp_policy -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd python_status_reports_elapsed_uptime_not_epoch_time -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd python_status_uses_configured_node_transfer_limits -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd python_status_uses_zero_acceptance_rate_before_offers -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd python_status_collapses_internal_peer_types_to_static_or_discovered -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd propagation_offer_ignores_control_allow_list_like_python -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd transport_bridge_regenerates_propagation_app_data_from_daemon_state -- --nocapture`
- `cargo test -p reticulum-rs-rpc --lib propagation -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd inbound_propagation_accepts_stamp_within_flexibility_window -- --nocapture`
- `cargo test -p reticulumd --bin reticulumd inbound_worker::tests -- --nocapture`
- `cargo test -p reticulumd --test lxmf_bridge_tests stamp -- --nocapture`
