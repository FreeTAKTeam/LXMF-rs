# Reticulum Parity Matrix

Status legend: not-started | partial | done

| Python Module | Rust Module | Status | Tests | Notes |
| --- | --- | --- | --- | --- |
| RNS/Reticulum.py | src/lib.rs + src/config.rs | partial | tests/config_parity.rs | init/config defaults |
| RNS/Identity.py | src/identity.rs | partial | tests/identity_parity.rs | identity serialization/signing |
| RNS/Destination.py | src/destination/* | partial | tests/destination_parity.rs, tests/lxmf_address_hash.rs | addressing/hash |
| RNS/Packet.py | src/packet.rs | partial | tests/packet_parity.rs, tests/lxmf_packet_limits.rs | framing/flags/limits |
| RNS/Transport.py | src/transport/* | partial | tests/transport_tables.rs | routing tables/links |
| RNS/Link.py | src/destination/link.rs | partial | tests/transport_tables.rs | link lifecycle/packets |
| RNS/Interfaces/* | src/iface/* | partial | tests/iface_parity.rs, tests/tcp_hdlc_test.rs | interface I/O |
| RNS/Cryptography/* | src/crypt/* + src/crypt.rs | partial | tests/lxmf_signature.rs, tests/lxmf_group_encrypt.rs | crypto/signatures |
| RNS/Resource.py | src/resource.rs | partial | tests/resource_channel_parity.rs | resource transfer |
| RNS/Channel.py | src/channel.rs | partial | tests/resource_channel_parity.rs | channels |
| RNS/Buffer.py | src/buffer.rs | partial | tests/buffer_parity.rs | buffer management |
| RNS/Discovery.py | src/transport/discovery.rs (missing) | not-started |  | discovery/announce |
| RNS/Resolver.py | src/utils/resolver.rs (missing) | not-started |  | resolver |
| RNS/Utilities/* | src/utils/* | partial | tests/hash_parity.rs | helpers/hashes |
| RNS/CRNS/* | src/bin/* (missing) | not-started |  | CLI tools |
