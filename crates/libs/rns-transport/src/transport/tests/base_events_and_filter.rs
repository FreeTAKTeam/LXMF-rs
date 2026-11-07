#[tokio::test]
async fn link_in_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let link_id = AddressHash::new_from_rand(OsRng);
    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"hello");

    assert!(
        transport
            .link_in_event_tx
            .send(LinkEventData {
                id: link_id,
                address_hash,
                event: LinkEvent::Data(Box::new(payload)),
                close_reason: None,
            })
            .is_ok(),
        "link input forwarder remains subscribed"
    );

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, link_id);
    assert_eq!(received.data.as_slice(), b"hello");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn link_out_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let link_id = AddressHash::new_from_rand(OsRng);
    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"outbound");

    assert!(
        transport
            .link_out_event_tx
            .send(LinkEventData {
                id: link_id,
                address_hash,
                event: LinkEvent::Data(Box::new(payload)),
                close_reason: None,
            })
            .is_ok(),
        "link output forwarder remains subscribed"
    );

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, link_id);
    assert_eq!(received.data.as_slice(), b"outbound");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn duplicate_membership_survives_proof_tracking_ttl() {
    let mut config: TransportConfig = Default::default();
    config.set_retransmit(true);

    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let _source1 = AddressHash::new_from_slice(&[1u8; 32]);
    let _source2 = AddressHash::new_from_slice(&[2u8; 32]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 32]);
    let destination = AddressHash::new_from_slice(&[4u8; 32]);

    let mut announce: Packet = Default::default();
    announce.header.header_type = HeaderType::Type2;
    announce.header.packet_type = PacketType::Announce;
    announce.header.hops = 3;
    announce.transport = Some(destination);

    assert!(handler.lock().await.filter_duplicate_packets(&announce).await);

    handle_announce(
        &announce,
        handler.lock().await,
        next_hop_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let data_packet: Packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"foo"),
        destination,
        ..Default::default()
    };
    let duplicate: Packet = data_packet.clone();

    let mut different_packet = data_packet.clone();
    different_packet.data = PacketDataBuffer::new_from_slice(b"bar");

    assert!(handler.lock().await.filter_duplicate_packets(&data_packet).await);
    assert!(!handler.lock().await.filter_duplicate_packets(&duplicate).await);
    assert!(handler.lock().await.filter_duplicate_packets(&different_packet).await);

    handler.lock().await.packet_cache.lock().await.release(Duration::ZERO);

    // Proof/source tracking expires independently of duplicate generations.
    assert!(!handler.lock().await.filter_duplicate_packets(&duplicate).await);
}

#[tokio::test]
async fn duplicate_resource_proofs_reach_resource_state_machine() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let proof = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        destination: AddressHash::new_from_slice(&[0x61; 32]),
        context: PacketContext::ResourceProof,
        data: PacketDataBuffer::new_from_slice(b"resource-proof"),
        ..Default::default()
    };

    assert!(handler.lock().await.filter_duplicate_packets(&proof).await);
    assert!(
        handler.lock().await.filter_duplicate_packets(&proof).await,
        "pinned Python bypasses duplicate filtering for RESOURCE_PRF packets"
    );
}

#[tokio::test]
async fn duplicate_resource_packets_pass_production_duplicate_filter() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        destination: AddressHash::new_from_slice(&[0x62; 32]),
        context: PacketContext::Resource,
        data: PacketDataBuffer::new_from_slice(b"resource-data"),
        ..Default::default()
    };

    assert!(handler.lock().await.filter_duplicate_packets(&packet).await);
    assert!(
        handler.lock().await.filter_duplicate_packets(&packet).await,
        "pinned Python bypasses duplicate filtering for RESOURCE packets"
    );
}
