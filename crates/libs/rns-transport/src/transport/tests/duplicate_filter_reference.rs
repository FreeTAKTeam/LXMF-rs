use super::*;
use crate::packet::{DestinationType, PacketContext};

/// This is the filter predicate's Cartesian domain, including invalid producer
/// shapes. Production ingress retains its separately tested stronger guards.
#[test]
#[ignore = "requires frozen Python checkout at RETICULUM_PY_REPO"]
fn frozen_python_all_21_contexts_match_ordered_generation_filter() {
    let repo = std::env::var("RETICULUM_PY_REPO").expect("frozen Python repository");
    let revision = std::process::Command::new("git")
        .args(["-C", &repo, "rev-parse", "HEAD"])
        .output()
        .expect("frozen revision");
    assert!(revision.status.success());
    assert_eq!(
        String::from_utf8_lossy(&revision.stdout).trim(),
        "99de23c040d507e3fefca19e87b182302902725d"
    );
    let script = r#"
from itertools import product
from types import SimpleNamespace
import RNS
from RNS.Transport import Transport
Transport.identity = SimpleNamespace(hash=b"\x01"*16)
contexts = list(range(0x0f))+list(range(0xfa,0x100))
results = []
for context, ptype, dtype, hops, tid, attached, member in product(contexts, range(4), range(4), range(3), range(3), range(2), range(3)):
    Transport.owner = SimpleNamespace(is_connected_to_shared_instance=bool(attached))
    Transport.packet_hashlist = {b"hash"} if member == 1 else set()
    Transport.packet_hashlist_prev = {b"hash"} if member == 2 else set()
    packet = SimpleNamespace(context=context, packet_type=ptype, destination_type=dtype,
        hops=hops, transport_id=[None,b"\x01"*16,b"\x02"*16][tid],
        packet_hash=b"hash", receiving_interface=None)
    results.append("1" if Transport.packet_filter(packet) else "0")
print("".join(results))
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env("PYTHONPATH", &repo)
        .output()
        .expect("execute frozen Python predicate");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let expected = String::from_utf8(output.stdout).expect("predicate bits");
    let expected = expected.trim().as_bytes();
    let mut index = 0;
    for context in (0_u8..0x0f).chain(0xfa..=0xff) {
        for packet_type in 0_u8..4 {
            for destination_type in 0_u8..4 {
                for hops in 0..3 {
                    for transport in 0..3 {
                        for attached in [false, true] {
                            for member in 0..3 {
                                let packet = Packet {
                                    header: crate::packet::Header {
                                        packet_type: PacketType::from(packet_type),
                                        destination_type: DestinationType::from(destination_type),
                                        hops,
                                        ..Default::default()
                                    },
                                    context: PacketContext::from(context),
                                    transport: match transport {
                                        0 => None,
                                        1 => Some(AddressHash::new(
                                            [1; crate::hash::ADDRESS_HASH_SIZE],
                                        )),
                                        _ => Some(AddressHash::new(
                                            [2; crate::hash::ADDRESS_HASH_SIZE],
                                        )),
                                    },
                                    ..Default::default()
                                };
                                let mut cache = PacketCache::new();
                                match member {
                                    1 => {
                                        cache.current.insert(packet.hash());
                                    }
                                    2 => {
                                        cache.previous.insert(packet.hash());
                                    }
                                    _ => {}
                                }
                                let accepted =
                                    crate::transport::inbound_processing::filter_duplicate_packet(
                                        &cache,
                                        &packet,
                                        hops,
                                        attached,
                                        AddressHash::new([1; crate::hash::ADDRESS_HASH_SIZE]),
                                    );
                                assert_eq!(u8::from(accepted) + b'0', expected[index],
                                    "context={context:02x} type={packet_type} destination={destination_type} hops={hops} transport={transport} attached={attached} membership={member}");
                                assert_eq!(
                                    cache.current.len(),
                                    usize::from(member == 1),
                                    "filter cannot promote/remember"
                                );
                                assert!(cache.map.is_empty());
                                index += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(index, 18_144);
    assert_eq!(expected.len(), index);
}
