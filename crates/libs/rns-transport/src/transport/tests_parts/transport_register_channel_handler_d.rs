#[tokio::test]
async fn transport_register_channel_handler_dispatches_inbound_channel_message() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    transport
        .register_channel_handler(&link_id, 0x4444, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        })
        .await
        .expect("register handler");

    let (_sequence, packet) = inbound
        .send_channel_message(0x4444, b"transport-channel".to_vec())
        .expect("channel message");
    handle_data(&packet, iface, handler.lock().await).await;

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].msg_type, 0x4444);
    assert_eq!(seen[0].payload, b"transport-channel");
}

#[tokio::test]
async fn transport_channel_message_state_tracks_delivery() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    inbound.register_channel_handler(0x55AA, |_| true);

    let (sequence, packet) = {
        let mut outbound = outbound.lock().await;
        outbound.send_channel_message(0x55AA, b"tracked".to_vec()).expect("channel message")
    };
    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Sent
    );

    let proof = match inbound.handle_packet(&packet, iface) {
        crate::destination::link::LinkHandleResult::Proof(proof) => proof,
        _ => panic!("channel packet should generate proof"),
    };
    {
        let mut outbound = outbound.lock().await;
        assert!(matches!(
            outbound.handle_packet(&proof, iface),
            crate::destination::link::LinkHandleResult::None
        ));
    }
    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Delivered
    );
}

#[tokio::test]
async fn transport_channel_handle_reports_missing_link() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let link_id = AddressHash::new_from_rand(OsRng);
    let channel = transport.channel(link_id);

    assert_eq!(channel.link_id(), link_id);
    assert!(matches!(
        channel.message_state(0).await,
        Err(crate::channel::ChannelError::LinkNotReady)
    ));
}

#[tokio::test]
async fn transport_channel_handle_supports_typed_messages() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let channel = transport.channel(link_id);

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    channel
        .register_typed_handler::<TestTypedMessage, _>(move |message| {
            seen_clone.lock().expect("lock").push(message);
            true
        })
        .await
        .expect("typed handler");

    let message = TestTypedMessage { value: b"typed-payload".to_vec() };
    let (_sequence, packet) = inbound
        .send_channel_message(TestTypedMessage::MSG_TYPE, message.encode())
        .expect("typed channel packet");
    handle_data(&packet, iface, handler.lock().await).await;

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.as_slice(), &[message]);
}

#[tokio::test]
async fn transport_channel_handle_can_remove_handlers() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let channel = transport.channel(link_id);

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    let handler_id = channel
        .register_handler(0x7777, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        })
        .await
        .expect("register handler");
    assert!(channel.remove_handler(handler_id).await.expect("remove handler"));
    assert!(!channel.remove_handler(handler_id).await.expect("remove handler twice"));

    let (_sequence, packet) =
        inbound.send_channel_message(0x7777, b"removed".to_vec()).expect("channel message");
    handle_data(&packet, iface, handler.lock().await).await;

    assert!(seen.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn transport_channel_handle_rejects_reserved_typed_messages_by_default() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let link_id = AddressHash::new_from_rand(OsRng);
    let channel = transport.channel(link_id);

    assert!(matches!(
        channel.register_typed_handler::<ReservedTypedMessage, _>(|_message| true).await,
        Err(ChannelError::InvalidMessageType)
    ));
}

#[tokio::test]
async fn transport_channel_handle_can_open_channel_without_handlers() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    let channel = transport.channel(link_id);
    channel.open().await.expect("open channel");

    let (_sequence, packet) =
        inbound.send_channel_message(0xEEEE, b"open-no-handler".to_vec()).expect("channel message");
    let result = outbound.lock().await.handle_packet(&packet, iface);
    assert!(matches!(result, crate::destination::link::LinkHandleResult::Proof(_)));
}

#[tokio::test]
async fn send_resource_returns_error_when_advertisement_dispatch_drops() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let mut resource_events = transport.resource_events();

    let result = transport.send_resource(&link_id, b"resource".to_vec(), None).await;
    assert!(matches!(result, Err(RnsError::ConnectionError)));

    let guard = handler.lock().await;
    assert!(guard.resource_manager.has_no_outbound_state());
    drop(guard);
    let event = timeout(Duration::from_millis(200), resource_events.recv())
        .await
        .expect("outbound failed event")
        .expect("resource event");
    assert_eq!(event.link_id, link_id);
    assert!(matches!(event.kind, ResourceEventKind::OutboundFailed));
}

#[tokio::test]
async fn cancel_resource_sends_initiator_cancel_and_removes_outbound_state() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut channel = transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(8, crate::iface::IfaceRole::Unicast);
    let iface = *channel.address();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    let mut resource_events = transport.resource_events();

    let resource_hash =
        transport.send_resource(&link_id, b"resource".to_vec(), None).await.expect("send resource");
    let advertised = timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("resource advertisement tx")
        .expect("resource advertisement message");
    assert_eq!(advertised.tx_type, TxMessageType::Direct(iface));
    assert_eq!(advertised.packet.context, PacketContext::ResourceAdvrtisement);

    let cancelled =
        transport.cancel_resource(&link_id, resource_hash).await.expect("cancel resource");
    assert!(cancelled);

    let cancel = timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("resource cancel tx")
        .expect("resource cancel message");
    assert_eq!(cancel.tx_type, TxMessageType::Direct(iface));
    assert_eq!(cancel.packet.destination, link_id);
    assert_eq!(cancel.packet.context, PacketContext::ResourceInitiatorCancel);
    let mut decrypted = PacketDataBuffer::new();
    let plain_len = {
        let outbound = outbound.lock().await;
        let plain = outbound
            .decrypt(cancel.packet.data.as_slice(), decrypted.accuire_buf_max())
            .expect("decrypt cancel packet");
        plain.len()
    };
    decrypted.resize(plain_len);
    assert_eq!(decrypted.as_slice(), resource_hash.as_slice());

    let mut guard = handler.lock().await;
    assert!(guard.resource_manager.has_no_outbound_state());
    let events = guard.resource_manager.drain_events();
    assert!(events.is_empty());
    drop(guard);
    let event = timeout(Duration::from_millis(200), resource_events.recv())
        .await
        .expect("cancel event")
        .expect("resource event");
    assert_eq!(event.hash, resource_hash);
    assert_eq!(event.link_id, link_id);
    assert!(matches!(event.kind, ResourceEventKind::OutboundCancelled));
}

// ---------------------------------------------------------------------
// Per-peer virtual unicast iface registration
// (see TransportHandler::unicast_iface_for_source)
// ---------------------------------------------------------------------
//
// On receiving an announce from a UDP peer over a multicast iface, the
// transport registers a *virtual* iface pinned to that peer's
// SocketAddr in the iface's PeerRouting map. The virtual iface shares
// its tx channel with the host multicast iface; the host's tx task
// resolves the virtual hash to a unicast send on the same socket.
// This is what stops the 22 Mb/s LAN flood without creating separate
// per-peer sockets (which would bind to ephemeral ports and confuse
// ingress attribution).

fn peer_addr(port: u16) -> std::net::SocketAddr {
    use std::net::{IpAddr, Ipv4Addr};
    std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 112)), port)
}

/// Register a fake multicast iface (role-tagged only — no real socket)
/// plus a shared `PeerRouting` map, and hand the routing map to the
/// handler so `unicast_iface_for_source` can use it. Returns the
/// iface's `AddressHash`.
///
/// Mirrors what `Transport::add_multicast_udp_interface` would do,
/// but without spawning the real UdpInterface task (which needs real
/// sockets). Tests can still exercise the handler's registration /
/// cache / GC logic in isolation this way.
async fn register_fake_multicast_iface(transport: &Transport) -> AddressHash {
    let routing = Arc::new(Mutex::new(crate::iface::udp::PeerRouting::new()));
    let iface_hash = {
        let mgr = transport.iface_manager();
        let mut mgr = mgr.lock().await;
        let channel = mgr.new_channel_with_role(16, crate::iface::IfaceRole::Multicast);
        *channel.address()
    };
    transport.get_handler().lock().await.register_multicast_peer_routing(iface_hash, routing);
    iface_hash
}

async fn new_unicast_iface_in(transport: &Transport) -> AddressHash {
    let mgr = transport.iface_manager();
    let mut mgr = mgr.lock().await;
    let channel = mgr.new_channel(16);
    *channel.address()
}
#[tokio::test]
async fn delivery_link_available_tracks_python_router_direct_and_backchannel_maps() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let outbound = Arc::new(Mutex::new(Link::new(destination, tx.clone())));

    assert!(!transport.delivery_link_available(&destination.address_hash).await);

    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    assert!(
        transport.delivery_link_available(&destination.address_hash).await,
        "pending direct links count as available like Python LXMRouter.direct_links membership"
    );

    outbound.lock().await.close();
    assert!(
        !transport.delivery_link_available(&destination.address_hash).await,
        "closed direct links must not leak availability while awaiting cleanup"
    );

    let inbound_request = Link::new(destination, tx.clone()).request();
    let inbound =
        Link::new_from_request(&inbound_request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let inbound = Arc::new(Mutex::new(inbound));
    let inbound_id = *inbound.lock().await.id();
    handler.lock().await.in_links.insert(inbound_id, inbound.clone());

    assert!(
        transport.delivery_link_available(&destination.address_hash).await,
        "backchannel links count as available like Python LXMRouter.backchannel_links membership"
    );

    inbound.lock().await.close();
    assert!(!transport.delivery_link_available(&destination.address_hash).await);
}
