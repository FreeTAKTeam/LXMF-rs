#[tokio::test]
async fn enabled_shared_daemon_delivers_local_link_request_only_to_requesting_child() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new(
        "enabled-shared-local-destination",
        &identity,
        true,
    ));
    let (mut parent_channel, requesting_child, sibling_child) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let parent_channel = manager.new_channel(8);
        let parent = *parent_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let requesting_child = manager
            .register_virtual_iface(parent, IfaceRole::Unicast)
            .expect("requesting local client");
        let sibling_child = manager
            .register_virtual_iface(parent, IfaceRole::Unicast)
            .expect("sibling local client");
        (parent_channel, requesting_child, sibling_child)
    };

    let destination = transport
        .add_destination(
            PrivateIdentity::new_from_rand(OsRng),
            DestinationName::new("lxmf", "enabled-shared-local"),
        )
        .await;
    let destination_desc = destination.lock().await.desc;
    let (link_events, _link_events_keepalive) = tokio::sync::broadcast::channel(4);
    let request = Link::new(destination_desc, link_events).request();
    let link_id = crate::destination::link::LinkId::from(&request);

    let inbound = RxMessage {
        address: requesting_child,
        packet: request,
        source: IfaceSource::None,
    };
    let (_, queued) = super::inbound_processing::preprocess_inbound_message(
        &transport.get_handler(),
        &transport.iface_messages_tx,
        inbound,
    )
    .await
    .expect("local-child LinkRequest should pass inbound admission");
    super::inbound_processing::process_inbound_message(transport.get_handler(), queued).await;

    let proof = parent_channel.tx_channel.try_recv().expect("one local LinkRequest proof");
    assert!(matches!(
        proof.tx_type,
        TxMessageType::Direct(iface) if iface == requesting_child
    ));
    assert_eq!(proof.packet.header.packet_type, PacketType::Proof);
    assert_eq!(proof.packet.header.destination_type, DestinationType::Link);
    assert_eq!(proof.packet.header.header_type, HeaderType::Type1);
    assert_eq!(proof.packet.header.propagation_type, PropagationType::Broadcast);
    assert_eq!(proof.packet.header.hops, 0);
    assert_eq!(proof.packet.context, PacketContext::LinkRequestProof);
    assert_eq!(proof.packet.destination, link_id);
    assert_eq!(proof.packet.transport, None);
    assert!(
        transport.get_handler().lock().await.in_links.contains_key(&link_id),
        "the locally hosted destination should accept the link"
    );
    assert!(
        matches!(
            parent_channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "transport-enabled local delivery must not broadcast or transit-forward to a sibling"
    );
    assert_ne!(requesting_child, sibling_child);
}
