# Reticulum Parity Matrix

Last verified: 2026-02-09 (`cargo fmt -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`)

Status legend: not-started | partial | done

| Python Module | Rust Module | Status | Tests | Notes |
| --- | --- | --- | --- | --- |
| `RNS/Reticulum.py` | `crates/reticulum/src/lib.rs` + `crates/reticulum/src/config.rs` | done | `crates/reticulum/tests/config_parity.rs` | Runtime initialization and config defaults are parity-covered. |
| `RNS/Identity.py` | `crates/reticulum/src/identity.rs` | done | `crates/reticulum/tests/identity_parity.rs`, `crates/reticulum/tests/lxmf_signature.rs` | Identity serialization/signing parity is fixture-covered. |
| `RNS/Destination.py` | `crates/reticulum/src/destination/*` | done | `crates/reticulum/tests/destination_parity.rs`, `crates/reticulum/tests/lxmf_address_hash.rs` | Destination addressing and hash derivation parity is covered. |
| `RNS/Packet.py` | `crates/reticulum/src/packet.rs` | done | `crates/reticulum/tests/packet_parity.rs`, `crates/reticulum/tests/lxmf_packet_limits.rs`, `crates/reticulum/tests/link_proof_packet.rs` | Packet framing, limits, and proof packet behavior are covered. |
| `RNS/Transport.py` | `crates/reticulum/src/transport/*` | done | `crates/reticulum/tests/transport_tables.rs`, `crates/reticulum/tests/transport_delivery.rs`, `crates/reticulum/tests/announce_scheduler.rs` | Routing, announce scheduling, and transport table mechanics are covered. |
| `RNS/Link.py` | `crates/reticulum/src/destination/link.rs` + `crates/reticulum/src/transport/link_table.rs` | done | `crates/reticulum/tests/link_event_layout.rs`, `crates/reticulum/tests/lxmf_receipt_callbacks.rs`, `crates/reticulum/tests/lxmf_receipt_proof.rs` | Link lifecycle/events and receipt flows are covered. |
| `RNS/Interfaces/*` | `crates/reticulum/src/iface/*` | done | `crates/reticulum/tests/iface_parity.rs`, `crates/reticulum/tests/tcp_hdlc_test.rs` | Interface framing/IO parity for supported interfaces is covered. |
| `RNS/Cryptography/*` | `crates/reticulum/src/crypt/*` + `crates/reticulum/src/crypt.rs` | done | `crates/reticulum/tests/crypto_parity.rs`, `crates/reticulum/tests/lxmf_group_encrypt.rs`, `crates/reticulum/tests/hash_parity.rs` | Core crypto and hash compatibility are fixture-tested. |
| `RNS/Resource.py` | `crates/reticulum/src/resource.rs` | done | `crates/reticulum/tests/resource_channel_parity.rs` | Resource advertisement/transfer channels are covered. |
| `RNS/Channel.py` | `crates/reticulum/src/channel.rs` | done | `crates/reticulum/tests/resource_channel_parity.rs` | Channel framing and interaction with resources are covered. |
| `RNS/Buffer.py` | `crates/reticulum/src/buffer.rs` | done | `crates/reticulum/tests/buffer_parity.rs` | Buffer management parity is covered by fixture tests. |
| `RNS/Discovery.py` | `crates/reticulum/src/transport/discovery.rs` | done | `crates/reticulum/tests/discovery_parity.rs`, `crates/reticulum/tests/python_announce.rs` | Discovery/announce behavior is covered with Python fixtures. |
| `RNS/Resolver.py` | `crates/reticulum/src/utils/resolver.rs` | done | `crates/reticulum/tests/resolver_parity.rs` | Resolver parity is covered for resolution paths. |
| `RNS/Utilities/*` | `crates/reticulum/src/utils/*` + `crates/reticulum/src/hash.rs` | done | `crates/reticulum/tests/hash_parity.rs`, `crates/reticulum/tests/api_helpers.rs` | Utility/helper parity is covered for exported helpers. |
| `RNS/CRNS/*` | `crates/reticulum/src/bin/*` | done | `crates/reticulum/tests/cli_parity.rs` | CLI tools and help/flag parity checks are covered. |
