# LXMF Parity Matrix

Status legend: not-started | partial | done

| Python Module | Rust Module | Status | Tests | Notes |
| --- | --- | --- | --- | --- |
| LXMF/LXMF.py | src/constants.rs + src/helpers.rs | done | tests/constants_parity.rs, tests/message_constants_parity.rs | constants/helpers |
| LXMF/LXMessage.py | src/message/* | partial | tests/message_payload_parity.rs, tests/message_wire_parity.rs, tests/message_storage_parity.rs, tests/message_object_parity.rs, tests/message_pack_parity.rs, tests/propagation_pack_parity.rs | payload/content/title/storage container; wire + propagation packing parity; remaining delivery/transport metadata |
| LXMF/LXMPeer.py | src/peer/mod.rs | partial | tests/peer_parity.rs, tests/peer.rs | peer tracking |
| LXMF/LXMRouter.py | src/router/mod.rs | partial | tests/router_parity.rs, tests/router_api.rs, tests/router_transport.rs, tests/propagation_parity.rs | router/transport/propagation |
| LXMF/Handlers.py | src/handlers.rs | partial | tests/handlers_parity.rs | handlers |
| LXMF/LXStamper.py | src/stamper.rs + src/ticket.rs | partial | tests/stamper_parity.rs, tests/stamp_parity.rs, tests/pn_stamp_parity.rs, tests/ticket_parity.rs | stamps + tickets (verification) |
| LXMF/Utilities/lxmd.py | src/bin/lxmd.rs | partial | tests/lxmd_cli.rs | daemon/cli |
