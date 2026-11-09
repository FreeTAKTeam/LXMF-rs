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

#[tokio::test]
async fn handle_data_keeps_each_outgoing_links_signal_isolated() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("signal-isolation", &identity, false));
    let iface = transport.iface_manager().lock().await.new_channel(8);
    let handler = transport.get_handler();
    let mut links = Vec::new();
    let mut peers = Vec::new();
    let mut events = Vec::new();
    for _ in 0..2 {
        let destination = SingleInputDestination::new(
            PrivateIdentity::new_from_rand(OsRng),
            DestinationName::new("lxmf", "delivery"),
        );
        let (tx, _) = tokio::sync::broadcast::channel(8);
        let mut outbound = Link::new(destination.desc, tx.clone());
        let request = outbound.request();
        let mut peer = Link::new_from_request(
            &request,
            destination.sign_key().clone(),
            destination.desc,
            tx.clone(),
        )
        .expect("valid link request");
        assert!(matches!(
            outbound.handle_packet(&peer.prove(), iface.address),
            crate::destination::link::LinkHandleResult::Activated
        ));
        outbound.set_ingress_iface(iface.address);
        events.push(tx.subscribe());
        let outbound = Arc::new(Mutex::new(outbound));
        handler.lock().await.out_links.insert(destination.desc.address_hash, outbound.clone());
        links.push(outbound);
        peers.push(peer);
    }
    let first_id = *links[0].lock().await.id();
    let second_id = *links[1].lock().await.id();
    assert_ne!(first_id, second_id, "independent established links");

    let signal_a = PacketSignal { rssi: Some(-98.0), snr: Some(7.5), q: Some(86.8) };
    let signal_b = PacketSignal { rssi: Some(-71.0), snr: Some(12.0), q: Some(93.0) };
    let signal_a_next = PacketSignal { rssi: Some(-94.0), snr: Some(8.0), q: None };
    let signal_b_next = PacketSignal { rssi: Some(-75.0), snr: None, q: Some(90.0) };
    let mut observed = Vec::new();
    for (index, signal, payload) in [
        (0, signal_a, b"first A".as_slice()),
        (1, signal_b, b"first B".as_slice()),
        (0, signal_a_next, b"next A".as_slice()),
        (1, signal_b_next, b"next B".as_slice()),
    ] {
        let packet = peers[index].data_packet(payload).expect("encrypted peer data");
        iface.packet_signals.record(packet.hash(), signal);
        handle_data(&packet, iface.address, handler.lock().await).await;

        let event = events[index].try_recv().expect("addressed link receives decrypted data");
        assert_eq!(event.id, *links[index].lock().await.id());
        assert_eq!(event.signal, Some(signal));
        let LinkEvent::Data(data) = event.event else {
            panic!("expected authenticated data event");
        };
        assert_eq!(data.as_slice(), payload);
        assert!(events[1 - index].try_recv().is_err(), "other link receives no data event");
        let first = links[0].lock().await.last_signal();
        let second = links[1].lock().await.last_signal();
        observed.push([first, second]);
    }
    assert_eq!(
        observed,
        [
            [Some(signal_a), None],
            [Some(signal_a), Some(signal_b)],
            [Some(signal_a_next), Some(signal_b)],
            [Some(signal_a_next), Some(signal_b_next)],
        ],
        "A's packets must not change B's RSSI/SNR/Q, and B's must not change A's"
    );
}
