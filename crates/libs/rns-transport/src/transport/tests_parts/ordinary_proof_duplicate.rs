fn ordinary_proof_packet() -> crate::packet::Packet {
    use crate::packet::{Packet, PacketContext, PacketType};

    Packet {
        header: crate::packet::Header {
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        context: PacketContext::None,
        destination: crate::hash::AddressHash::new_from_slice(&[0x91; 16]),
        data: crate::packet::PacketDataBuffer::new_from_slice(&[0x37; 32]),
        ..Default::default()
    }
}

#[tokio::test]
async fn rns_1_5_ingress_suppresses_duplicate_ordinary_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("proof-replay", &local_identity, true));
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let packet = ordinary_proof_packet();

    let first = preprocess_inbound_message(
        &transport.get_handler(),
        &transport.iface_messages_tx,
        crate::iface::RxMessage {
            address: iface,
            packet: packet.clone(),
            source: Default::default(),
        },
    )
    .await;
    assert!(first.is_some(), "first ordinary proof should pass production ingress");

    let duplicate = preprocess_inbound_message(
        &transport.get_handler(),
        &transport.iface_messages_tx,
        crate::iface::RxMessage { address: iface, packet, source: Default::default() },
    )
    .await;
    assert!(duplicate.is_none(), "exact replayed ordinary proof must be filtered");
}

#[tokio::test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
async fn pinned_python_ordinary_proof_filter_matches_production_ingress() {
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
Transport.packet_hashlist = set()
Transport.packet_hashlist_prev = set()
packet_hash = b"ordinary-proof-replay"
packet = SimpleNamespace(transport_id=None, context=RNS.Packet.NONE,
                         destination_type=RNS.Destination.SINGLE,
                         packet_type=RNS.Packet.PROOF, hops=0,
                         packet_hash=packet_hash)
first = Transport.packet_filter(packet)
Transport.add_packet_hash(packet_hash)
duplicate = Transport.packet_filter(packet)
print(f"{int(first)},{int(duplicate)}")
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env(
            "PYTHONPATH",
            format!("{python_repo}:{}", std::env::var("PYTHONPATH").unwrap_or_default()),
        )
        .output()
        .expect("run pinned Python ordinary-proof duplicate differential");
    assert!(
        output.status.success(),
        "pinned Python filter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1,0");

    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("proof-replay", &local_identity, true));
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let packet = ordinary_proof_packet();
    for (index, expected) in [true, false].into_iter().enumerate() {
        let accepted = preprocess_inbound_message(
            &transport.get_handler(),
            &transport.iface_messages_tx,
            crate::iface::RxMessage {
                address: iface,
                packet: packet.clone(),
                source: Default::default(),
            },
        )
        .await
        .is_some();
        assert_eq!(accepted, expected, "Rust production ingress observation {index}");
    }
}
