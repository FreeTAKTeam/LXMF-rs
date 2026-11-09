use super::*;

#[tokio::test]
async fn a_packet_heard_with_a_signal_delivers_it_with_the_data() {
    let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
    let receiver_transport =
        Transport::new(TransportConfig::new("receiver", &receiver_identity, true));
    let receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(8);
    let own_destination = receiver_transport
        .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let own_hash = own_destination.lock().await.desc.address_hash;
    let mut received = receiver_transport.received_data_events();

    let sender_identity = PrivateIdentity::new_from_rand(OsRng);
    let sender_transport = Transport::new(TransportConfig::new("sender", &sender_identity, true));
    let mut sender_iface = sender_transport.iface_manager().lock().await.new_channel(8);
    let receiver_announce =
        own_destination.lock().await.announce(OsRng, None).expect("valid announce");
    handle_announce(
        &receiver_announce,
        sender_transport.get_handler().lock().await,
        sender_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;
    let outbound = Packet {
        destination: own_hash,
        data: PacketDataBuffer::new_from_slice(b"heard at -98 dBm"),
        ..Packet::default()
    };
    sender_transport.send_packet_with_trace(outbound).await;
    let sent = timeout(Duration::from_millis(200), sender_iface.tx_channel.recv())
        .await
        .expect("packet queued for the sender's interface")
        .expect("tx channel open");

    let signal = PacketSignal { rssi: Some(-98.0), snr: Some(7.5), q: Some(86.8) };
    receiver_iface.packet_signals.record(sent.packet.hash(), signal);
    handle_data(
        &sent.packet,
        receiver_iface.address,
        receiver_transport.get_handler().lock().await,
    )
    .await;

    let delivered = timeout(Duration::from_millis(200), received.recv())
        .await
        .expect("delivered")
        .expect("received data channel open");
    assert_eq!(delivered.signal, Some(signal), "the bearer's reading rides with the packet");
    assert_eq!(
        receiver_transport.packet_signal(&sent.packet.hash()).await,
        Some(signal),
        "and answers the management query for its hash"
    );
}

#[test]
fn a_link_keeps_the_last_signal_a_bearer_reported() {
    let destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let (link_events, _) = tokio::sync::broadcast::channel(4);
    let mut link = Link::new(destination.desc, link_events);
    assert_eq!(link.last_signal(), None);

    link.note_signal(Some(PacketSignal { rssi: Some(-90.0), snr: Some(4.0), q: None }));
    link.note_signal(None);

    assert_eq!(
        link.last_signal().and_then(|signal| signal.rssi),
        Some(-90.0),
        "a packet with no reading leaves the last"
    );
}
