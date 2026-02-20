# LXMF Parity Matrix

Last verified: 2026-02-19 (`cargo fmt -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-features`)

Status legend: `not-started` | `partial` | `done`.

`done` means a behavior-level parity expectation is implemented and gated by this repo's active suites or migration evidence.

## Module Map

| Python Module | Rust Module | Status |
| --- | --- | --- |
| `LXMF/LXMF.py` | `crates/libs/lxmf-core` | done |
| `LXMF/LXMessage.py` | `crates/libs/lxmf-core` | done |
| `LXMF/LXMPeer.py` | `crates/libs/lxmf-sdk` | done |
| `LXMF/LXMRouter.py` | `crates/libs/rns-rpc` | done |
| `LXMF/Handlers.py` | `crates/apps/reticulumd` + `crates/libs/rns-rpc` | done |
| `LXMF/LXStamper.py` | `crates/libs/lxmf-core` | done |

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
- PARITY_ITEM id=stamper.validate_pn_stamp status=done
- PARITY_ITEM id=stamper.generate_stamp status=done
- PARITY_ITEM id=stamper.cancel_work status=done
- PARITY_ITEM id=ticket.validity_with_grace status=done
- PARITY_ITEM id=ticket.renewal_window status=done
- PARITY_ITEM id=ticket.derived_stamp status=done
- PARITY_ITEM id=peer.serialize_roundtrip status=done
- PARITY_ITEM id=peer.queue_accounting status=done
- PARITY_ITEM id=peer.acceptance_rate status=done
- PARITY_ITEM id=peer.peering_key status=done
- PARITY_ITEM id=router.outbound_queue status=done
- PARITY_ITEM id=router.handle_outbound_policy status=done
- PARITY_ITEM id=router.adapter_transport status=done
- PARITY_ITEM id=router.paper_uri_ingest status=done
- PARITY_ITEM id=router.cancel_outbound status=done
- PARITY_ITEM id=router.propagation_ingest_fetch status=done
- PARITY_ITEM id=router.transfer_state_lifecycle status=done
- PARITY_ITEM id=router.node_app_data status=done
- PARITY_ITEM id=handlers.delivery_callback status=done
- PARITY_ITEM id=handlers.propagation_app_data status=done
- PARITY_ITEM id=handlers.router_side_effects status=done
- PARITY_ITEM id=interop.python_live_gate status=done
