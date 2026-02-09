# LXMF Parity Matrix

Last verified: 2026-02-09 (`cargo fmt -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`)

Status legend: `not-started` | `partial` | `done`.

`done` means a behavior-level test exists and is listed in `tests=...`.

## Module Map

| Python Module | Rust Module | Status |
| --- | --- | --- |
| `LXMF/LXMF.py` | `src/constants.rs`, `src/helpers.rs` | done |
| `LXMF/LXMessage.py` | `src/message/*` | partial |
| `LXMF/LXMPeer.py` | `src/peer/mod.rs` | partial |
| `LXMF/LXMRouter.py` | `src/router/mod.rs` | partial |
| `LXMF/Handlers.py` | `src/handlers.rs` | partial |
| `LXMF/LXStamper.py` | `src/stamper.rs`, `src/ticket.rs` | partial |
| `LXMF/Utilities/lxmd.py` | `src/lxmd/*`, `src/bin/lxmd.rs` | partial |

## Required Method-Level Checklist

- PARITY_ITEM id=message.pack_wire status=done tests=tests/message_pack_parity.rs
- PARITY_ITEM id=message.unpack_wire status=done tests=tests/message_wire_parity.rs
- PARITY_ITEM id=message.storage_roundtrip status=done tests=tests/message_storage_parity.rs
- PARITY_ITEM id=message.propagation_pack_unpack status=done tests=tests/propagation_pack_parity.rs,tests/propagation_unpack_parity.rs
- PARITY_ITEM id=message.paper_pack status=done tests=tests/paper_pack_parity.rs
- PARITY_ITEM id=message.paper_uri_helpers status=done tests=tests/message_uri_file_helpers.rs
- PARITY_ITEM id=message.file_unpack_helpers status=done tests=tests/message_uri_file_helpers.rs
- PARITY_ITEM id=message.signature_verify status=done tests=tests/message_signature.rs
- PARITY_ITEM id=message.object_accessors status=done tests=tests/message_object_parity.rs
- PARITY_ITEM id=stamper.validate_pn_stamp status=done tests=tests/pn_stamp_parity.rs
- PARITY_ITEM id=stamper.generate_stamp status=done tests=tests/stamper_ticket_behavior.rs
- PARITY_ITEM id=stamper.cancel_work status=done tests=tests/stamper_ticket_behavior.rs
- PARITY_ITEM id=ticket.validity_with_grace status=done tests=tests/stamper_ticket_behavior.rs
- PARITY_ITEM id=ticket.renewal_window status=done tests=tests/stamper_ticket_behavior.rs
- PARITY_ITEM id=ticket.derived_stamp status=done tests=tests/stamper_ticket_behavior.rs
- PARITY_ITEM id=peer.serialize_roundtrip status=done tests=tests/peer_behavior.rs
- PARITY_ITEM id=peer.queue_accounting status=done tests=tests/peer_behavior.rs
- PARITY_ITEM id=peer.acceptance_rate status=done tests=tests/peer_behavior.rs
- PARITY_ITEM id=peer.peering_key status=done tests=tests/peer_behavior.rs
- PARITY_ITEM id=router.outbound_queue status=done tests=tests/router_api.rs,tests/router_parity.rs
- PARITY_ITEM id=router.handle_outbound_policy status=done tests=tests/router_behavior.rs
- PARITY_ITEM id=router.adapter_transport status=done tests=tests/router_transport.rs
- PARITY_ITEM id=router.paper_uri_ingest status=done tests=tests/router_paper_uri.rs
- PARITY_ITEM id=router.cancel_outbound status=done tests=tests/router_behavior.rs
- PARITY_ITEM id=router.propagation_ingest_fetch status=done tests=tests/router_propagation.rs
- PARITY_ITEM id=router.transfer_state_lifecycle status=done tests=tests/router_behavior.rs
- PARITY_ITEM id=router.node_app_data status=done tests=tests/propagation_node_app_data_parity.rs,tests/propagation_node_app_data_custom_parity.rs
- PARITY_ITEM id=handlers.delivery_callback status=done tests=tests/handlers_behavior.rs
- PARITY_ITEM id=handlers.propagation_app_data status=done tests=tests/handlers_behavior.rs
- PARITY_ITEM id=handlers.router_side_effects status=done tests=tests/handlers_parity.rs
- PARITY_ITEM id=lxmd.config_load status=done tests=tests/lxmd_runtime.rs
- PARITY_ITEM id=lxmd.serve_sync_unpeer_status status=done tests=tests/lxmd_runtime.rs
- PARITY_ITEM id=lxmd.live_daemon_runtime status=done tests=tests/lxmd_runtime.rs,tests/lxmd_cli.rs
- PARITY_ITEM id=interop.python_live_gate status=done tests=tests/python_interop_gate.rs
