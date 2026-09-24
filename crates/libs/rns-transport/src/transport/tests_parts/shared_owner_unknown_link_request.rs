#[tokio::test]
async fn unknown_owner_local_link_request_uses_only_shared_iface_and_records_return_route() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-owner-link", &local_identity, true);
    config.set_transport_enabled(true);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);

    let (mut ingress, mut shared_owner, mut owner_server, mut ordinary) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let ingress = manager.new_channel_with_role_mode_mtu(
            8,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            640,
        );
        let shared_owner = manager.new_channel_with_role_mode_mtu(
            8,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            500,
        );
        let owner_server = manager.new_channel(8);
        assert!(manager.set_shared_instance(*owner_server.address(), true));
        assert!(manager.set_outgoing(*owner_server.address(), false));
        let ordinary = manager.new_channel_with_role_mode_mtu(
            8,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            900,
        );
        assert!(manager.set_shared_instance(*shared_owner.address(), true));
        (ingress, shared_owner, owner_server, ordinary)
    };
    let ingress_iface = *ingress.address();
    let shared_iface = *shared_owner.address();
    let owner_server_iface = *owner_server.address();

    let owner_identity = PrivateIdentity::new_from_rand(OsRng);
    let identity = *owner_identity.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (events, _keepalive) = tokio::sync::broadcast::channel(4);
    let mut request = Link::new(destination, events).request();
    request.header.header_type = HeaderType::Type2;
    request.header.propagation_type = crate::packet::PropagationType::Transport;
    request.transport = Some(*local_identity.address_hash());
    assert_eq!(request.header.packet_type, PacketType::LinkRequest);
    {
        let handler = transport.handler.lock().await;
        assert!(handler.config.transport_enabled);
        assert!(handler.config.connected_to_shared_instance);
        assert!(handler.iface_manager.lock().await.is_shared_instance(&shared_iface));
        assert!(handler.iface_manager.lock().await.is_shared_instance(&owner_server_iface));
    }

    super::path::handle_link_request(&request, ingress_iface, transport.handler.lock().await).await;

    let forwarded = shared_owner.tx_channel.try_recv().expect("one upstream LinkRequest");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(shared_iface));
    assert_eq!(forwarded.packet.destination, destination.address_hash);
    assert_eq!(forwarded.packet.header.packet_type, PacketType::LinkRequest);
    assert_eq!(forwarded.packet.header.header_type, HeaderType::Type1);
    assert_eq!(forwarded.packet.transport, None);
    let signalling = &forwarded.packet.data.as_slice()[forwarded.packet.data.len() - 3..];
    let advertised_mtu = ((signalling[0] as usize) << 16)
        | ((signalling[1] as usize) << 8)
        | signalling[2] as usize;
    assert_eq!(advertised_mtu & 0x1f_ffff, 500, "upstream MTU clamps the link request");
    assert!(matches!(ingress.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert!(matches!(owner_server.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert!(matches!(ordinary.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));

    let mut handler = transport.handler.lock().await;
    let link_id = LinkId::from(&forwarded.packet);
    assert_eq!(handler.link_table.proof_validation_context(&link_id), Some((destination.address_hash, shared_iface)));
    let proof = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        context: PacketContext::LinkRequestProof,
        destination: link_id,
        data: PacketDataBuffer::new_from_slice(b"proof"),
        ..Default::default()
    };
    let (returned_proof, return_iface) = handler.link_table.handle_proof(&proof).expect("recorded return route");
    assert_eq!(return_iface, ingress_iface);
    assert_eq!(returned_proof.destination, link_id);
    assert_eq!(handler.link_table.original_destination(&link_id), Some(destination.address_hash));
}

#[tokio::test]
async fn ordinary_unknown_link_request_stays_dropped_when_not_shared_connected() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("ordinary-unknown-link", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let (mut ingress, mut ordinary) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(8), manager.new_channel(8))
    };
    let ingress_iface = *ingress.address();

    let identity = *PrivateIdentity::new_from_rand(OsRng).as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (events, _keepalive) = tokio::sync::broadcast::channel(4);
    let mut request = Link::new(destination, events).request();
    request.header.header_type = HeaderType::Type2;
    request.header.propagation_type = crate::packet::PropagationType::Transport;
    request.transport = Some(*local_identity.address_hash());

    super::path::handle_link_request(&request, ingress_iface, transport.handler.lock().await).await;

    assert!(matches!(ingress.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert!(matches!(ordinary.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert_eq!(transport.handler.lock().await.link_table.len(), 0);
}

#[tokio::test]
async fn unknown_owner_local_link_request_is_not_reflected_to_shared_ingress() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-ingress-link", &local_identity, true);
    config.set_transport_enabled(true);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let (mut shared_ingress, mut ordinary) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let shared_ingress = manager.new_channel(8);
        assert!(manager.set_shared_instance(*shared_ingress.address(), true));
        (shared_ingress, manager.new_channel(8))
    };
    let ingress_iface = *shared_ingress.address();
    let identity = *PrivateIdentity::new_from_rand(OsRng).as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (events, _keepalive) = tokio::sync::broadcast::channel(4);
    let mut request = Link::new(destination, events).request();
    request.header.header_type = HeaderType::Type2;
    request.header.propagation_type = crate::packet::PropagationType::Transport;
    request.transport = Some(*local_identity.address_hash());

    super::path::handle_link_request(&request, ingress_iface, transport.handler.lock().await).await;

    assert!(matches!(shared_ingress.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert!(matches!(ordinary.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert_eq!(transport.handler.lock().await.link_table.len(), 0);
}
