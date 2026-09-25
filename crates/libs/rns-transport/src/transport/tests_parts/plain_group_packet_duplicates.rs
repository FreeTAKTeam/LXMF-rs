fn unicast_plain_or_group_packet(destination_type: DestinationType) -> Packet {
    Packet {
        header: Header {
            packet_type: PacketType::Data,
            destination_type,
            hops: 0,
            ..Default::default()
        },
        destination: AddressHash::new_from_slice(&[0x62; 16]),
        data: PacketDataBuffer::new_from_slice(b"same unicast payload"),
        ..Default::default()
    }
}

#[tokio::test]
async fn rns_1_5_plain_and_group_packets_bypass_hash_duplicate_filter() {
    let transport = Transport::new(TransportConfig::default());
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();

    for destination_type in [DestinationType::Plain, DestinationType::Group] {
        let packet = unicast_plain_or_group_packet(destination_type);
        for replay in 0..2 {
            let queued = preprocess_inbound_message(
                &transport.get_handler(),
                &transport.iface_messages_tx,
                crate::iface::RxMessage {
                    address: iface,
                    packet: packet.clone(),
                    source: Default::default(),
                },
            )
            .await;
            assert!(
                queued.is_some(),
                "Python accepts the duplicate {destination_type:?} packet at replay {replay}"
            );
        }
    }
}

#[tokio::test]
async fn plain_group_packet_for_another_transport_is_rejected_before_duplicate_bypass() {
    let transport = Transport::new(TransportConfig::default());
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let other_transport = AddressHash::new_from_slice(&[0x93; 16]);

    for destination_type in [DestinationType::Plain, DestinationType::Group] {
        let mut packet = unicast_plain_or_group_packet(destination_type);
        packet.transport = Some(other_transport);
        let queued = preprocess_inbound_message(
            &transport.get_handler(),
            &transport.iface_messages_tx,
            crate::iface::RxMessage { address: iface, packet, source: Default::default() },
        )
        .await;
        assert!(queued.is_none(), "wrong transport identity must reject {destination_type:?}");
    }
}

#[tokio::test]
async fn shared_instance_duplicate_bypass_precedes_mismatched_transport_rejection() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-client-wrong-transport", &local_identity, false);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let other_transport = AddressHash::new_from_slice(&[0x93; 16]);

    for destination_type in [DestinationType::Plain, DestinationType::Group] {
        let mut packet = unicast_plain_or_group_packet(destination_type);
        packet.transport = Some(other_transport);
        for replay in 0..2 {
            let queued = preprocess_inbound_message(
                &transport.get_handler(),
                &transport.iface_messages_tx,
                crate::iface::RxMessage {
                    address: iface,
                    packet: packet.clone(),
                    source: Default::default(),
                },
            )
            .await;
            assert!(
                queued.is_some(),
                "shared-instance client delegates mismatched-identity {destination_type:?} replay {replay} to its owner"
            );
        }
    }
}

#[tokio::test]
#[ignore = "requires pinned Python Reticulum checkout at RETICULUM_PY_REPO"]
async fn pinned_python_plain_group_duplicate_filter_matches_production_ingress() {
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
Transport.owner = SimpleNamespace(is_connected_to_shared_instance=False,
                                  identity=None)
Transport.identity = SimpleNamespace(hash=b"\x01" * 16)
Transport.packet_hashlist = set()
Transport.packet_hashlist_prev = set()
for destination_type in (RNS.Destination.PLAIN, RNS.Destination.GROUP):
    packet = SimpleNamespace(transport_id=None, context=RNS.Packet.NONE,
                             destination_type=destination_type,
                             packet_type=RNS.Packet.DATA, hops=0,
                             packet_hash=b"same-unicast-packet")
    first = Transport.packet_filter(packet)
    Transport.add_packet_hash(packet.packet_hash)
    second = Transport.packet_filter(packet)
    if not first or not second:
        raise AssertionError((destination_type, first, second))
print("plain=1,1;group=1,1")

for destination_type in (RNS.Destination.PLAIN, RNS.Destination.GROUP):
    packet = SimpleNamespace(transport_id=b"\x02" * 16, context=RNS.Packet.NONE,
                             destination_type=destination_type,
                             packet_type=RNS.Packet.DATA, hops=0,
                             packet_hash=b"wrong-transport-packet")
    if Transport.packet_filter(packet):
        raise AssertionError((destination_type, "wrong transport accepted"))
print("wrong-transport=0,0")

Transport.owner.is_connected_to_shared_instance = True
Transport.packet_hashlist = set()
Transport.packet_hashlist_prev = set()
for destination_type in (RNS.Destination.PLAIN, RNS.Destination.GROUP):
    packet = SimpleNamespace(transport_id=b"\x02" * 16, context=RNS.Packet.NONE,
                             destination_type=destination_type,
                             packet_type=RNS.Packet.DATA, hops=0,
                             packet_hash=b"shared-wrong-transport-packet")
    first = Transport.packet_filter(packet)
    Transport.add_packet_hash(packet.packet_hash)
    second = Transport.packet_filter(packet)
    if not first or not second:
        raise AssertionError((destination_type, "shared instance rejected wrong-transport duplicate"))
print("shared-wrong-transport=1,1")
"#;
    let python = std::env::var("LXMF_PYTHON_BIN").unwrap_or_else(|_| "python3".to_string());
    let output = std::process::Command::new(python)
        .args(["-c", script])
        .env(
            "PYTHONPATH",
            format!("{python_repo}:{}", std::env::var("PYTHONPATH").unwrap_or_default()),
        )
        .output()
        .expect("run pinned Python Plain/Group duplicate differential");
    assert!(
        output.status.success(),
        "pinned Python filter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "plain=1,1;group=1,1\nwrong-transport=0,0\nshared-wrong-transport=1,1"
    );

    let transport = Transport::new(TransportConfig::default());
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    for destination_type in [DestinationType::Plain, DestinationType::Group] {
        let packet = unicast_plain_or_group_packet(destination_type);
        for expected in [true, true] {
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
            assert_eq!(accepted, expected, "Rust {destination_type:?} packet filter");
        }
    }
    let other_transport = AddressHash::new_from_slice(&[0x93; 16]);
    for destination_type in [DestinationType::Plain, DestinationType::Group] {
        let mut packet = unicast_plain_or_group_packet(destination_type);
        packet.transport = Some(other_transport);
        let accepted = preprocess_inbound_message(
            &transport.get_handler(),
            &transport.iface_messages_tx,
            crate::iface::RxMessage { address: iface, packet, source: Default::default() },
        )
        .await
        .is_some();
        assert!(!accepted, "Rust must reject wrong-transport {destination_type:?} packets");
    }
}
