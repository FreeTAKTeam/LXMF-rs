#[tokio::test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
async fn pinned_python_duplicate_filter_context_matrix_matches_rust() {
    const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let python_repo = std::env::var("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the pinned Python Reticulum checkout");
    let revision = std::process::Command::new("git")
        .args(["-C", &python_repo, "rev-parse", "HEAD"])
        .output()
        .expect("read pinned Python Reticulum revision");
    assert!(revision.status.success(), "git rev-parse failed for {python_repo}");
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PINNED_RETICULUM);

    let script = r#"
from types import SimpleNamespace
import RNS
from RNS.Transport import Transport
Transport.owner = SimpleNamespace(is_connected_to_shared_instance=False)
Transport.identity = SimpleNamespace(hash=b"\x01" * 16)
Transport.packet_hashlist = set()
Transport.packet_hashlist_prev = set()
cases = [
    ("data", RNS.Packet.DATA, RNS.Packet.NONE),
    ("link_request", RNS.Packet.LINKREQUEST, RNS.Packet.NONE),
    ("proof", RNS.Packet.PROOF, RNS.Packet.NONE),
    ("keepalive", RNS.Packet.DATA, RNS.Packet.KEEPALIVE),
    ("resource_request", RNS.Packet.DATA, RNS.Packet.RESOURCE_REQ),
    ("resource_proof", RNS.Packet.PROOF, RNS.Packet.RESOURCE_PRF),
    ("resource", RNS.Packet.DATA, RNS.Packet.RESOURCE),
    ("cache_request", RNS.Packet.DATA, RNS.Packet.CACHE_REQUEST),
    ("channel", RNS.Packet.DATA, RNS.Packet.CHANNEL),
    ("announce", RNS.Packet.ANNOUNCE, RNS.Packet.NONE),
]
results = []
for name, packet_type, context in cases:
    packet_hash = name.encode()
    packet = SimpleNamespace(transport_id=None, context=context,
        destination_type=RNS.Destination.SINGLE, packet_type=packet_type,
        hops=0, packet_hash=packet_hash)
    first = Transport.packet_filter(packet)
    Transport.add_packet_hash(packet_hash)
    second = Transport.packet_filter(packet)
    results.append(f"{name}={int(first)},{int(second)}")
print(";".join(results))
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env(
            "PYTHONPATH",
            format!("{python_repo}:{}", std::env::var("PYTHONPATH").unwrap_or_default()),
        )
        .output()
        .expect("run pinned Python duplicate-filter matrix");
    assert!(
        output.status.success(),
        "pinned Python filter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "data=1,0;link_request=1,0;proof=1,0;keepalive=1,1;resource_request=1,1;resource_proof=1,1;resource=1,1;cache_request=1,1;channel=1,1;announce=1,1"
    );

    use crate::packet::{Packet, PacketContext, PacketType};
    let rust_cases = [
        (PacketType::Data, PacketContext::None, false),
        (PacketType::LinkRequest, PacketContext::None, false),
        (PacketType::Proof, PacketContext::None, false),
        (PacketType::Data, PacketContext::KeepAlive, true),
        (PacketType::Data, PacketContext::ResourceRequest, true),
        (PacketType::Proof, PacketContext::ResourceProof, true),
        (PacketType::Data, PacketContext::Resource, true),
        (PacketType::Data, PacketContext::CacheRequest, true),
        (PacketType::Data, PacketContext::Channel, true),
        (PacketType::Announce, PacketContext::None, true),
    ];
    for (index, (packet_type, context, duplicate_admitted)) in rust_cases.into_iter().enumerate() {
        let transport = Transport::new(TransportConfig::default());
        let handler = transport.get_handler();
        let packet = Packet {
            header: crate::packet::Header { packet_type, ..Default::default() },
            context,
            destination: AddressHash::new_from_slice(&[0x70 + index as u8; 16]),
            data: crate::packet::PacketDataBuffer::new_from_slice(&[index as u8; 8]),
            ..Default::default()
        };
        assert!(handler.lock().await.filter_duplicate_packets(&packet).await);
        assert_eq!(
            handler.lock().await.filter_duplicate_packets(&packet).await,
            duplicate_admitted,
            "Rust duplicate-filter case {index}: {packet_type:?}/{context:?}"
        );
    }
}

#[tokio::test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
async fn pinned_python_packet_hash_rotation_and_size_contract() {
    const PINNED_RETICULUM: &str = "99de23c040d507e3fefca19e87b182302902725d";
    let python_repo = std::env::var("RETICULUM_PY_REPO")
        .expect("set RETICULUM_PY_REPO to the pinned Python Reticulum checkout");
    let revision = std::process::Command::new("git")
        .args(["-C", &python_repo, "rev-parse", "HEAD"])
        .output()
        .expect("read pinned Python Reticulum revision");
    assert!(revision.status.success(), "git rev-parse failed for {python_repo}");
    assert_eq!(String::from_utf8_lossy(&revision.stdout).trim(), PINNED_RETICULUM);

    let script = r#"
from types import SimpleNamespace
import RNS
from RNS.Transport import Transport
Transport.owner = SimpleNamespace(is_connected_to_shared_instance=False)
Transport.identity = SimpleNamespace(hash=b"\x01" * 16)
Transport.packet_hashlist = set()
Transport.packet_hashlist_prev = set()
Transport.hashlist_maxsize = 4
def packet(value):
    return SimpleNamespace(transport_id=None, context=RNS.Packet.NONE,
        destination_type=RNS.Destination.SINGLE, packet_type=RNS.Packet.DATA,
        hops=0, packet_hash=value)
def rotate_if_needed():
    if len(Transport.packet_hashlist) > Transport.hashlist_maxsize // 2:
        Transport.packet_hashlist_prev = Transport.packet_hashlist
        Transport.packet_hashlist = set()
for value in (b"a", b"b", b"c"):
    Transport.add_packet_hash(value)
rotate_if_needed()
first_generation = (len(Transport.packet_hashlist_prev), len(Transport.packet_hashlist))
old_hash_admitted = Transport.packet_filter(packet(b"a"))
for value in (b"d", b"e", b"f"):
    Transport.add_packet_hash(value)
rotate_if_needed()
second_generation = (len(Transport.packet_hashlist_prev), len(Transport.packet_hashlist))
previous_generation_admitted = Transport.packet_filter(packet(b"d"))
expired_generation_admitted = Transport.packet_filter(packet(b"a"))
print(f"max={Transport.hashlist_maxsize};generations={first_generation[0]},{first_generation[1]}|{second_generation[0]},{second_generation[1]};checks={int(old_hash_admitted)},{int(previous_generation_admitted)},{int(expired_generation_admitted)}")
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env(
            "PYTHONPATH",
            format!("{python_repo}:{}", std::env::var("PYTHONPATH").unwrap_or_default()),
        )
        .output()
        .expect("run pinned Python packet-hash rotation contract");
    assert!(
        output.status.success(),
        "pinned Python cache contract failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "max=4;generations=3,0|3,0;checks=0,0,1"
    );

    // This assertion records the current Rust expiry contract separately: Rust
    // has time-based release, not Python's two-generation size-based rotation.
    let mut rust_cache = super::packet_cache::PacketCache::new();
    let packets: Vec<_> = (0..6)
        .map(|index| crate::packet::Packet {
            data: crate::packet::PacketDataBuffer::new_from_slice(&[index]),
            ..Default::default()
        })
        .collect();
    for packet in &packets {
        assert!(rust_cache.update(packet));
    }
    assert_eq!(rust_cache.hashes().len(), 6);
}
