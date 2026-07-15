#[tokio::test]
async fn observed_send_exposes_final_encrypted_packet_hash_before_dispatch() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("proof-correlation", &local_identity, true);
    let transport = Transport::new(config);
    let mut iface_channel = transport.iface_manager().lock().await.new_channel(16);
    let iface = *iface_channel.address();

    let mut remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let announce = remote_destination.announce(OsRng, None).expect("valid announce");
    let destination = announce.destination;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let packet = Packet {
        destination,
        data: PacketDataBuffer::new_from_slice(b"correlate this delivery proof"),
        ..Packet::default()
    };
    let original_hash = packet.hash();
    let observed_hash = Arc::new(StdMutex::new(None));
    let observer_value = observed_hash.clone();

    let trace = transport
        .send_packet_observed_with_trace(packet, move |packet_hash| {
            *observer_value.lock().expect("observer hash lock") = Some(packet_hash);
        })
        .await;

    assert_eq!(trace.outcome, SendPacketOutcome::SentDirect);
    let trace_hash = trace.packet_hash.expect("prepared packet hash");
    assert_ne!(trace_hash, original_hash, "encryption must change the packet identity");
    assert_eq!(*observed_hash.lock().expect("observer hash lock"), Some(trace_hash));

    let sent = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("packet should be queued")
        .expect("tx channel open");
    assert_eq!(sent.packet.hash(), trace_hash);

    let receipt = DeliveryReceipt::new(trace_hash.to_bytes());
    assert_eq!(receipt.packet_hash(), trace_hash);
    assert!(receipt.matches_packet_hash(&trace_hash));

    let ordinary_packet = Packet {
        destination,
        data: PacketDataBuffer::new_from_slice(b"return this finalized packet hash"),
        ..Packet::default()
    };
    let ordinary_trace = transport.send_packet_with_trace(ordinary_packet).await;
    let ordinary_sent = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("ordinary packet should be queued")
        .expect("tx channel open");
    assert_eq!(ordinary_trace.packet_hash, Some(ordinary_sent.packet.hash()));
}

#[tokio::test]
async fn observed_send_has_no_hash_when_packet_preparation_fails() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("proof-correlation", &local_identity, true);
    let transport = Transport::new(config);
    let observed = Arc::new(AtomicUsize::new(0));
    let observer_value = observed.clone();
    let packet = Packet {
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"missing destination identity"),
        ..Packet::default()
    };

    let trace = transport
        .send_packet_observed_with_trace(packet, move |_| {
            observer_value.fetch_add(1, Ordering::SeqCst);
        })
        .await;

    assert_eq!(trace.outcome, SendPacketOutcome::DroppedMissingDestinationIdentity);
    assert_eq!(trace.packet_hash, None);
    assert_eq!(observed.load(Ordering::SeqCst), 0);
}
