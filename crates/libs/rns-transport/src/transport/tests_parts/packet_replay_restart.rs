#[tokio::test]
async fn packet_hashlist_restores_proof_replay_filter_after_restart() {
    use crate::packet::{Packet, PacketContext, PacketType};

    let identity = PrivateIdentity::new_from_rand(OsRng);
    let make_transport = || {
        let mut config = TransportConfig::new("packet-replay-restart", &identity, true);
        config.set_retransmit(true);
        Transport::new(config)
    };
    let packet = Packet {
        header: crate::packet::Header { packet_type: PacketType::Proof, ..Default::default() },
        context: PacketContext::None,
        destination: AddressHash::new([0x91; crate::hash::ADDRESS_HASH_SIZE]),
        data: crate::packet::PacketDataBuffer::new_from_slice(&[0x37; crate::hash::HASH_SIZE]),
        ..Default::default()
    };
    let temp = tempfile::tempdir().expect("transport storage");
    let before_restart = make_transport();
    let ingress = *before_restart.iface_manager().lock().await.new_channel(8).address();

    let first = preprocess_inbound_message(
        &before_restart.get_handler(),
        &before_restart.iface_messages_tx,
        crate::iface::RxMessage {
            address: ingress,
            packet: packet.clone(),
            source: Default::default(),
        },
    )
    .await;
    assert!(first.is_some(), "first production-ingress proof should be accepted");
    assert_eq!(before_restart.save_packet_hashlist(temp.path()).await.expect("persist cache"), 1);
    let persisted = std::fs::read(temp.path().join("packet_hashlist.raw")).expect("Python-format cache");
    assert_eq!(persisted, packet.hash().as_slice());

    let after_restart = make_transport();
    assert_eq!(after_restart.restore_packet_hashlist(temp.path()).await.expect("restore cache"), 1);
    let replay = preprocess_inbound_message(
        &after_restart.get_handler(),
        &after_restart.iface_messages_tx,
        crate::iface::RxMessage { address: ingress, packet, source: Default::default() },
    )
    .await;
    assert!(replay.is_none(), "exact replayed proof must remain filtered after transport restart");
}

#[tokio::test]
async fn packet_hashlist_restore_skips_shared_instance_clients() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-packet-cache", &identity, true);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let temp = tempfile::tempdir().expect("transport storage");
    tokio::fs::write(temp.path().join("packet_hashlist.raw"), [0x5a; crate::hash::HASH_SIZE])
        .await
        .expect("write cached hash");

    assert_eq!(transport.restore_packet_hashlist(temp.path()).await.expect("skip shared cache"), 0);
}
