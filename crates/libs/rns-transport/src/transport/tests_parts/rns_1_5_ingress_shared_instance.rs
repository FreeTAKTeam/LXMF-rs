#[tokio::test]
async fn rns_1_5_shared_instance_client_defers_duplicate_filtering_to_owner() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-client", &local_identity, false);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let iface = *manager.new_channel(8).address();
        assert!(manager.set_shared_instance(iface, true));
        iface
    };
    let packet = Packet {
        header: Header {
            destination_type: crate::packet::DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        context: crate::packet::PacketContext::None,
        destination: AddressHash::new_from_slice(&[0x9A; crate::hash::ADDRESS_HASH_SIZE]),
        data: PacketDataBuffer::new_from_slice(b"shared-instance duplicate"),
        ..Default::default()
    };

    for arrival in 1..=2 {
        assert!(
            preprocess_inbound_message(
                &handler,
                &transport.iface_messages_tx,
                crate::iface::RxMessage {
                    address: iface,
                    packet: packet.clone(),
                    source: crate::iface::IfaceSource::None,
                },
            )
            .await
            .is_some(),
            "the shared-instance owner filters duplicate #{arrival}"
        );
    }
}
