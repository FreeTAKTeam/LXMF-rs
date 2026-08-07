use crate::iface::InterfaceMode;
use crate::packet::PropagationType;

#[tokio::test]
async fn unknown_path_request_is_answered_when_matching_announce_arrives() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let (mut learned_channel, mut requester_channel) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(16), manager.new_channel(16))
    };
    let learned_iface = *learned_channel.address();
    let requester_iface = *requester_channel.address();
    assert!(transport
        .iface_manager()
        .lock()
        .await
        .set_mode(requester_iface, InterfaceMode::AccessPoint));

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 3;
    let destination = announce.destination;
    let cached_data = announce.data.clone();

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x91; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, requester_iface).await;
    }

    let recursive = timeout(Duration::from_millis(200), learned_channel.tx_channel.recv())
        .await
        .expect("recursive path request should be forwarded")
        .expect("recursive path request message");
    assert!(
        matches!(recursive.tx_type, TxMessageType::Broadcast(Some(iface)) if iface == requester_iface),
        "recursive discovery request should exclude the original requester iface"
    );
    assert_eq!(recursive.packet.destination, path_request.destination);
    assert_eq!(recursive.packet.data.as_slice(), path_request.data.as_slice());
    assert!(matches!(
        requester_channel.tx_channel.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let response = timeout(Duration::from_millis(200), requester_channel.tx_channel.recv())
        .await
        .expect("matching announce should answer waiting discovery request")
        .expect("path response message");
    assert!(
        matches!(response.tx_type, TxMessageType::Direct(iface) if iface == requester_iface),
        "waiting discovery path responses should be direct to the original requester"
    );
    assert_eq!(response.packet.destination, destination);
    assert_eq!(response.packet.header.header_type, HeaderType::Type2);
    assert_eq!(response.packet.header.propagation_type, PropagationType::Transport);
    assert_eq!(response.packet.header.hops, 3);
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    assert_eq!(response.packet.transport, Some(*local_identity.address_hash()));
    assert_eq!(response.packet.data.as_slice(), cached_data.as_slice());

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert!(
        matches!(
            requester_channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "a consumed discovery request should not be answered again"
    );
}

#[tokio::test]
async fn matched_unknown_path_announce_releases_discovery_capacity() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let (mut learned_channel, mut requester_channel) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(16), manager.new_channel(16))
    };
    let learned_iface = *learned_channel.address();
    let requester_iface = *requester_channel.address();
    {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        assert!(manager.set_mode(requester_iface, InterfaceMode::AccessPoint));
        assert!(manager.set_announce_pacing(requester_iface, 0, 0));
    }

    {
        let mut guard = handler.lock().await;
        guard.path_requests = super::path_requests::PathRequests::new(
            "test",
            Some(*local_identity.address_hash()),
            1,
            1,
            30,
        );
    }

    let mut first_remote = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let first_announce = first_remote.announce(OsRng, None).expect("valid announce packet");
    let first_destination = first_announce.destination;
    let first_request = {
        let mut guard = handler.lock().await;
        guard
            .path_requests
            .generate(&first_destination, Some(vec![0x81; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&first_request, &mut guard, requester_iface).await;
    }
    timeout(Duration::from_millis(200), learned_channel.tx_channel.recv())
        .await
        .expect("first discovery request should be forwarded")
        .expect("first recursive discovery message");

    let blocked_destination =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"blocked-while-pending"));
    let blocked_request = {
        let mut guard = handler.lock().await;
        guard
            .path_requests
            .generate(&blocked_destination, Some(vec![0x82; crate::hash::ADDRESS_HASH_SIZE]))
    };
    {
        let mut guard = handler.lock().await;
        handle_path_request(&blocked_request, &mut guard, requester_iface).await;
    }
    assert!(
        matches!(
            learned_channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "per-interface discovery capacity should block a second pending unknown request"
    );

    handle_announce(
        &first_announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    timeout(Duration::from_millis(200), requester_channel.tx_channel.recv())
        .await
        .expect("matching announce should answer and consume the pending request")
        .expect("path response message");

    let released_destination =
        AddressHash::new_from_hash(&Hash::new_from_slice(b"allowed-after-consume"));
    let released_request = {
        let mut guard = handler.lock().await;
        guard
            .path_requests
            .generate(&released_destination, Some(vec![0x83; crate::hash::ADDRESS_HASH_SIZE]))
    };
    {
        let mut guard = handler.lock().await;
        handle_path_request(&released_request, &mut guard, requester_iface).await;
    }

    let released_recursive = timeout(Duration::from_millis(200), learned_channel.tx_channel.recv())
        .await
        .expect("consumed discovery request should release recursive capacity")
        .expect("released recursive discovery message");
    assert!(
        matches!(released_recursive.tx_type, TxMessageType::Broadcast(Some(iface)) if iface == requester_iface),
        "new recursive discovery should still exclude the original requester iface"
    );
    assert_eq!(released_recursive.packet.data.as_slice(), released_request.data.as_slice());
}

#[tokio::test]
async fn unknown_path_recursive_discovery_obeys_python_interface_modes() {
    for mode in [InterfaceMode::AccessPoint, InterfaceMode::Gateway, InterfaceMode::Roaming] {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut config = TransportConfig::new("test", &local_identity, true);
        config.set_retransmit(true);
        let transport = Transport::new(config);
        let handler = transport.get_handler();

        let (mut learned_channel, requester_iface) = {
            let manager = transport.iface_manager();
            let mut manager = manager.lock().await;
            let learned_channel = manager.new_channel(16);
            let requester_channel = manager.new_channel(16);
            let requester_iface = *requester_channel.address();
            assert!(manager.set_mode(requester_iface, mode));
            (learned_channel, requester_iface)
        };

        let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"allowed-unknown"));
        let path_request = {
            let mut guard = handler.lock().await;
            guard
                .path_requests
                .generate(&destination, Some(vec![0xAA; crate::hash::ADDRESS_HASH_SIZE]))
        };

        {
            let mut guard = handler.lock().await;
            handle_path_request(&path_request, &mut guard, requester_iface).await;
        }

        let recursive = timeout(Duration::from_millis(200), learned_channel.tx_channel.recv())
            .await
            .expect("allowed mode should forward recursive unknown-path discovery")
            .expect("recursive unknown-path discovery");
        assert!(
            matches!(recursive.tx_type, TxMessageType::Broadcast(Some(iface)) if iface == requester_iface),
            "{mode:?} should forward recursive discovery excluding the requester"
        );
    }

    for mode in [InterfaceMode::Full, InterfaceMode::PointToPoint, InterfaceMode::Boundary] {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut config = TransportConfig::new("test", &local_identity, true);
        config.set_retransmit(true);
        let transport = Transport::new(config);
        let handler = transport.get_handler();

        let (mut learned_channel, mut requester_channel, requester_iface) = {
            let manager = transport.iface_manager();
            let mut manager = manager.lock().await;
            let learned_channel = manager.new_channel(16);
            let requester_channel = manager.new_channel(16);
            let requester_iface = *requester_channel.address();
            assert!(manager.set_mode(requester_iface, mode));
            (learned_channel, requester_channel, requester_iface)
        };

        let remote_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut remote_destination =
            SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
        let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
        let destination = announce.destination;
        let path_request = {
            let mut guard = handler.lock().await;
            guard
                .path_requests
                .generate(&destination, Some(vec![0xAA; crate::hash::ADDRESS_HASH_SIZE]))
        };

        {
            let mut guard = handler.lock().await;
            handle_path_request(&path_request, &mut guard, requester_iface).await;
        }

        assert!(
            matches!(
                learned_channel.tx_channel.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "{mode:?} must not trigger recursive unknown-path discovery"
        );

        if mode != InterfaceMode::Boundary {
            handle_announce(
                &announce,
                handler.lock().await,
                *learned_channel.address(),
                crate::iface::IfaceSource::None,
            )
            .await;
            assert!(matches!(
                requester_channel.tx_channel.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ));
        }
    }
}

#[tokio::test]
async fn boundary_unknown_discovery_is_forwarded_only_to_boundary_and_gateway_ifaces() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("boundary-discovery", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (mut boundary_channel, mut gateway_channel, mut full_channel, requester_channel) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let boundary = manager.new_channel_with_role_and_mode(
            16,
            crate::iface::IfaceRole::Unicast,
            InterfaceMode::Boundary,
        );
        let gateway = manager.new_channel_with_role_and_mode(
            16,
            crate::iface::IfaceRole::Unicast,
            InterfaceMode::Gateway,
        );
        let full = manager.new_channel_with_role_and_mode(
            16,
            crate::iface::IfaceRole::Unicast,
            InterfaceMode::Full,
        );
        let requester = manager.new_channel_with_role_and_mode(
            16,
            crate::iface::IfaceRole::Unicast,
            InterfaceMode::Boundary,
        );
        (boundary, gateway, full, requester)
    };
    let requester_iface = *requester_channel.address();
    let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"boundary-unknown"));
    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0xB1; crate::hash::ADDRESS_HASH_SIZE]))
    };

    handle_path_request(&path_request, &mut handler.lock().await, requester_iface).await;

    timeout(Duration::from_millis(200), boundary_channel.tx_channel.recv())
        .await
        .expect("boundary interface should receive recursive discovery")
        .expect("boundary recursive discovery packet");
    timeout(Duration::from_millis(200), gateway_channel.tx_channel.recv())
        .await
        .expect("gateway interface should receive recursive discovery")
        .expect("gateway recursive discovery packet");
    assert!(matches!(
        full_channel.tx_channel.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}
