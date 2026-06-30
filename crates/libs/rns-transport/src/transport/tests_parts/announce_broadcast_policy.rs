async fn recv_rebroadcast(
    channel: &mut crate::iface::InterfaceChannel,
    label: &str,
) -> crate::iface::TxMessage {
    timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
        .unwrap_or_else(|| panic!("iface closed while waiting for {label}"))
}

fn assert_ordinary_rebroadcast(message: &crate::iface::TxMessage, source: &Packet) {
    assert_eq!(message.packet.destination, source.destination);
    assert_eq!(message.packet.context, PacketContext::None);
    assert_eq!(message.packet.header.header_type, HeaderType::Type2);
    assert_eq!(
        message.packet.header.propagation_type,
        PropagationType::Broadcast
    );
    assert_eq!(message.packet.data.as_slice(), source.data.as_slice());
    assert!(
        message.packet.transport.is_some(),
        "rebroadcast should stamp the local transport id"
    );
}

fn no_rebroadcast(channel: &mut crate::iface::InterfaceChannel, label: &str) {
    assert!(
        matches!(
            channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "{label} should not receive announce rebroadcast"
    );
}

async fn new_mode_channel(
    transport: &Transport,
    mode: crate::iface::InterfaceMode,
) -> crate::iface::InterfaceChannel {
    transport.iface_manager().lock().await.new_channel_with_role_and_mode(
        16,
        crate::iface::IfaceRole::Unicast,
        mode,
    )
}

async fn learn_announce_on_iface(
    transport: &Transport,
    iface: crate::hash::AddressHash,
    aspect: &str,
) -> Packet {
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", aspect),
    );
    let announce = destination.announce(OsRng, None).expect("announce");
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    announce
}

async fn release_due_rebroadcasts(transport: &Transport) {
    tokio::time::sleep(Duration::from_millis(550)).await;
    super::announce::retransmit_announces(transport.get_handler().lock().await).await;
}

#[tokio::test]
async fn transport_announce_rebroadcast_policy_uses_learned_next_hop_iface_mode() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);

    let mut learned_full = new_mode_channel(&transport, crate::iface::InterfaceMode::Full).await;
    let mut access_point =
        new_mode_channel(&transport, crate::iface::InterfaceMode::AccessPoint).await;
    let mut roaming = new_mode_channel(&transport, crate::iface::InterfaceMode::Roaming).await;
    let mut boundary = new_mode_channel(&transport, crate::iface::InterfaceMode::Boundary).await;

    let full_announce =
        learn_announce_on_iface(&transport, *learned_full.address(), "fanout-full").await;
    release_due_rebroadcasts(&transport).await;

    no_rebroadcast(&mut learned_full, "learned full ingress");
    no_rebroadcast(&mut access_point, "access point");
    let roaming_rebroadcast = recv_rebroadcast(&mut roaming, "roaming rebroadcast").await;
    let boundary_rebroadcast = recv_rebroadcast(&mut boundary, "boundary rebroadcast").await;
    assert_ordinary_rebroadcast(&roaming_rebroadcast, &full_announce);
    assert_ordinary_rebroadcast(&boundary_rebroadcast, &full_announce);

    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test-roaming", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let mut learned_roaming =
        new_mode_channel(&transport, crate::iface::InterfaceMode::Roaming).await;
    let mut full = new_mode_channel(&transport, crate::iface::InterfaceMode::Full).await;
    let mut blocked_boundary =
        new_mode_channel(&transport, crate::iface::InterfaceMode::Boundary).await;

    let roaming_announce =
        learn_announce_on_iface(&transport, *learned_roaming.address(), "fanout-roaming").await;
    release_due_rebroadcasts(&transport).await;

    no_rebroadcast(&mut learned_roaming, "learned roaming ingress");
    let full_rebroadcast = recv_rebroadcast(&mut full, "full rebroadcast").await;
    assert_ordinary_rebroadcast(&full_rebroadcast, &roaming_announce);
    no_rebroadcast(
        &mut blocked_boundary,
        "boundary with roaming learned next-hop",
    );

    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test-boundary", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let mut learned_boundary =
        new_mode_channel(&transport, crate::iface::InterfaceMode::Boundary).await;
    let mut full = new_mode_channel(&transport, crate::iface::InterfaceMode::Full).await;
    let mut blocked_roaming =
        new_mode_channel(&transport, crate::iface::InterfaceMode::Roaming).await;

    let boundary_announce =
        learn_announce_on_iface(&transport, *learned_boundary.address(), "fanout-boundary").await;
    release_due_rebroadcasts(&transport).await;

    no_rebroadcast(&mut learned_boundary, "learned boundary ingress");
    let full_rebroadcast = recv_rebroadcast(&mut full, "full rebroadcast").await;
    assert_ordinary_rebroadcast(&full_rebroadcast, &boundary_announce);
    no_rebroadcast(
        &mut blocked_roaming,
        "roaming with boundary learned next-hop",
    );
}
