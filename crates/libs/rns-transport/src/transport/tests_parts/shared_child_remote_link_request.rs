fn shared_child_remote_request(packet: &mut Packet, transport_id: AddressHash) {
    packet.header.header_type = HeaderType::Type2;
    packet.header.propagation_type = crate::packet::PropagationType::Transport;
    packet.transport = Some(transport_id);
}

#[tokio::test]
async fn enabled_shared_child_routes_remote_link_request_only_to_next_hop() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("enabled-child-remote", &identity, true));
    let (mut parent_channel, child, sibling, mut remote_channel) = {
        let mut manager = transport.iface_manager.lock().await;
        let parent_channel = manager.new_channel(8);
        let parent = *parent_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let child = manager.register_virtual_iface(parent, IfaceRole::Unicast).expect("child");
        let sibling = manager.register_virtual_iface(parent, IfaceRole::Unicast).expect("sibling");
        let remote_channel = manager.new_channel(8);
        (parent_channel, child, sibling, remote_channel)
    };
    let remote_iface = *remote_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let identity_desc = *remote_identity.as_identity();
    let remote_destination = identity_desc.address_hash;
    transport.handler.lock().await.path_table.restore_tunnel_path(
        remote_destination,
        remote_destination,
        1,
        remote_iface,
        crate::hash::Hash::new_from_slice(b"remote-route"),
        std::time::Instant::now(),
    );
    let desc = DestinationDesc {
        identity: identity_desc,
        address_hash: identity_desc.address_hash,
        name: DestinationName::new("lxmf", "remote"),
    };
    let (events, _keepalive) = tokio::sync::broadcast::channel(4);
    let mut packet = Link::new(desc, events).request();
    shared_child_remote_request(&mut packet, *identity.address_hash());

    super::path::handle_link_request(&packet, child, transport.handler.lock().await).await;

    let forwarded = remote_channel.tx_channel.try_recv().expect("one next-hop transmission");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(remote_iface));
    assert_eq!(forwarded.packet.destination, remote_destination);
    assert_eq!(forwarded.packet.header.packet_type, PacketType::LinkRequest);
    assert_eq!(forwarded.packet.header.header_type, HeaderType::Type1);
    assert_eq!(forwarded.packet.transport, None);
    assert!(matches!(parent_channel.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert_ne!(child, sibling);
}

#[tokio::test]
async fn disabled_shared_child_routes_remote_link_request_only_to_next_hop() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("disabled-child-remote", &identity, false);
    config.set_transport_enabled(false);
    let transport = Transport::new(config);
    let (mut parent_channel, child, sibling, mut remote_channel) = {
        let mut manager = transport.iface_manager.lock().await;
        let parent_channel = manager.new_channel(8);
        let parent = *parent_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let child = manager.register_virtual_iface(parent, IfaceRole::Unicast).expect("child");
        let sibling = manager.register_virtual_iface(parent, IfaceRole::Unicast).expect("sibling");
        let remote_channel = manager.new_channel(8);
        (parent_channel, child, sibling, remote_channel)
    };
    let remote_iface = *remote_channel.address();
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let identity_desc = *remote_identity.as_identity();
    let remote_destination = identity_desc.address_hash;
    transport.handler.lock().await.path_table.restore_tunnel_path(
        remote_destination,
        remote_destination,
        1,
        remote_iface,
        crate::hash::Hash::new_from_slice(b"remote-route"),
        std::time::Instant::now(),
    );
    let desc = DestinationDesc {
        identity: identity_desc,
        address_hash: identity_desc.address_hash,
        name: DestinationName::new("lxmf", "remote"),
    };
    let (events, _keepalive) = tokio::sync::broadcast::channel(4);
    let mut packet = Link::new(desc, events).request();
    shared_child_remote_request(&mut packet, *identity.address_hash());

    super::path::handle_link_request(&packet, child, transport.handler.lock().await).await;

    let forwarded = remote_channel.tx_channel.try_recv().expect("one next-hop transmission");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(remote_iface));
    assert_eq!(forwarded.packet.destination, remote_destination);
    assert_eq!(forwarded.packet.header.packet_type, PacketType::LinkRequest);
    assert_eq!(forwarded.packet.header.header_type, HeaderType::Type1);
    assert_eq!(forwarded.packet.transport, None);
    assert!(matches!(parent_channel.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)));
    assert_ne!(child, sibling);
}
