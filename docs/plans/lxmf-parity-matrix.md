# LXMF Parity Matrix

Status legend: not-started | partial | done

| Python Module | Rust Module | Status | Tests | Notes |
| --- | --- | --- | --- | --- |
| LXMF/LXMF.py | src/constants.rs + src/helpers.rs | done | tests/constants_parity.rs | constants/helpers |
| LXMF/LXMessage.py | src/message/* | partial | tests/payload_parity.rs, tests/wire_parity.rs | payload + wire |
| LXMF/LXMPeer.py | src/peer/mod.rs | partial | tests/peer_parity.rs | peer tracking |
| LXMF/LXMRouter.py | src/router/mod.rs | partial | tests/router_parity.rs | router |
| LXMF/Handlers.py | src/handlers.rs | partial | tests/handlers_parity.rs | handlers |
| LXMF/LXStamper.py | src/stamper.rs + src/ticket.rs | partial | tests/stamper_parity.rs, tests/stamp_parity.rs, tests/pn_stamp_parity.rs, tests/ticket_parity.rs | stamps + tickets (verification) |
| LXMF/Utilities/lxmd.py | src/bin/lxmd.rs | partial | tests/lxmd_cli.rs | daemon/cli |
