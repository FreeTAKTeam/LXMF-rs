#[tokio::test]
async fn enabled_shared_daemon_does_not_transit_announce_for_local_destination() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new(
        "enabled-local-announce",
        &identity,
        true,
    ));
    let (mut parent_channel, local_client, sibling_client) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let parent_channel = manager.new_channel(8);
        let parent = *parent_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let local_client = manager
            .register_virtual_iface(parent, IfaceRole::Unicast)
            .expect("requesting local client");
        let sibling_client = manager
            .register_virtual_iface(parent, IfaceRole::Unicast)
            .expect("sibling local client");
        (parent_channel, local_client, sibling_client)
    };

    let destination = transport
        .add_destination(
            identity,
            DestinationName::new("lxmf", "enabled-local-announce"),
        )
        .await;
    let announce = destination
        .lock()
        .await
        .announce(OsRng, None)
        .expect("local announce");
    let destination_hash = announce.destination;
    let inbound = RxMessage {
        address: local_client,
        packet: announce,
        source: IfaceSource::None,
    };
    let (_, queued) = super::inbound_processing::preprocess_inbound_message(
        &transport.get_handler(),
        &transport.iface_messages_tx,
        inbound,
    )
    .await
    .expect("valid local-client announce should reach production handling");
    super::inbound_processing::process_inbound_message(transport.get_handler(), queued).await;

    let handler = transport.get_handler();
    let handler = handler.lock().await;
    assert!(
        handler.path_table.get(&destination_hash).is_none(),
        "a locally hosted destination must not be learned as a remote path"
    );
    assert_eq!(handler.announce_table.tier_sizes(), (0, 0));
    drop(handler);
    assert!(
        matches!(
            parent_channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "a transport-enabled shared daemon must not transit the announce to a sibling client"
    );
    assert_ne!(local_client, sibling_client);
}
