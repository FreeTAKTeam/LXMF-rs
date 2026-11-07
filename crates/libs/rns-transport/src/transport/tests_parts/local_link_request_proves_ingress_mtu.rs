// A link request to a local destination may signal more than the ingress
// interface carries. The proof confirms the capped value, since every send on
// the link is sized by it.

async fn responder_with_ingress_mtu(mtu: usize) -> (Transport, crate::iface::InterfaceChannel, DestinationDesc) {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("responder", &identity, true));
    let ingress = transport.iface_manager().lock().await.new_channel_with_role_mode_mtu(
        8,
        crate::iface::IfaceRole::Unicast,
        crate::iface::InterfaceMode::Full,
        mtu,
    );
    let server = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("nomadnetwork", "node"),
    );
    let desc = server.desc;
    transport
        .get_handler()
        .lock()
        .await
        .single_in_destinations
        .insert(desc.address_hash, Arc::new(Mutex::new(server)));
    (transport, ingress, desc)
}

/// Both ends after the proof: the initiator's negotiated MTU and the
/// responder's stored in-link.
async fn negotiated_mtus(ingress_mtu: usize, signalled_mtu: usize) -> (usize, usize) {
    let (responder, mut ingress, desc) = responder_with_ingress_mtu(ingress_mtu).await;
    let ingress_iface = *ingress.address();
    let (link_events, _keep) = tokio::sync::broadcast::channel(4);
    let mut initiator = Link::new(desc, link_events);
    let request = initiator.request_with_mtu(signalled_mtu);

    super::path::handle_link_request(&request, ingress_iface, responder.get_handler().lock().await).await;

    let proof = ingress.tx_channel.recv().await.expect("the responder proves the link").packet;
    assert!(matches!(
        initiator.handle_packet(&proof, ingress_iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
    let in_link = responder
        .get_handler()
        .lock()
        .await
        .in_links
        .values()
        .next()
        .cloned()
        .expect("the responder keeps the in-link");
    let responder_mtu = in_link.lock().await.link_mtu();
    (initiator.link_mtu(), responder_mtu)
}

#[tokio::test]
async fn local_link_request_is_proven_at_the_ingress_interface_mtu() {
    assert_eq!(negotiated_mtus(500, 262_144).await, (500, 500));
}

#[tokio::test]
async fn local_link_request_within_the_ingress_interface_mtu_keeps_its_value() {
    assert_eq!(negotiated_mtus(262_144, 1196).await, (1196, 1196));
}
