#[tokio::test]
async fn held_udp_announce_preserves_peer_source_for_unicast_route() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;
    let iface = register_fake_multicast_iface(&transport).await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let first_peer = peer_addr(4242);
    let mut first_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let first_announce = first_destination.announce(OsRng, None).expect("first announce");
    handle_announce(
        &first_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::Udp(first_peer),
    )
    .await;
    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");

    for idx in 0..3 {
        let mut packet = Packet::default();
        packet.header.packet_type = PacketType::Announce;
        packet.header.hops = 15;
        packet.destination = AddressHash::new([0xA0 + idx; crate::hash::ADDRESS_HASH_SIZE]);
        let _ = handler.lock().await.announce_limits.check(
            iface,
            &packet,
            crate::iface::IfaceSource::None,
            false,
        );
    }

    let held_peer = peer_addr(5252);
    let mut held_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let held_announce = held_destination.announce(OsRng, None).expect("held announce");
    handle_announce(
        &held_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::Udp(held_peer),
    )
    .await;
    assert!(matches!(
        announce_rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    let mut released = None;
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        release_held_announces(handler.lock().await).await;
        if let Ok(event) = timeout(Duration::from_millis(80), announce_rx.recv()).await {
            released = Some(event.expect("broadcast receive"));
            break;
        }
    }
    let released = released.expect("held announce should release");

    let guard = handler.lock().await;
    let virtual_hash = guard
        .unicast_udp_ifaces
        .get(&held_peer)
        .map(|(hash, _)| *hash)
        .expect("held peer should register virtual iface");
    assert_eq!(released.interface, virtual_hash.as_slice().to_vec());

    let routing = guard.multicast_peer_routings.get(&iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&held_peer), Some(virtual_hash));
}

#[tokio::test]
async fn learned_announces_are_not_held_after_route_is_known() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = AddressHash::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let announce = destination.announce(OsRng, None).expect("announce");

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;
    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");

    tokio::time::sleep(Duration::from_millis(5)).await;
    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let repeated = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("known announce should bypass ingress hold")
        .expect("broadcast receive");
    assert_eq!(repeated.hops, announce.header.hops);
}

#[tokio::test]
async fn path_response_announces_are_not_held_by_rate_limits() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 1,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 0.0,
        burst_freq: 0.0,
        burst_hold: Duration::from_secs(60),
        burst_penalty: Duration::from_secs(60),
        held_release_interval: Duration::from_secs(60),
    });

    let iface = AddressHash::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "propagation"),
    );
    let mut announce = destination.announce(OsRng, None).expect("announce");
    announce.context = PacketContext::PathResponse;

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let received = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("path response announce should emit immediately")
        .expect("broadcast receive");
    assert_eq!(received.destination.lock().await.desc.address_hash, announce.destination);
}

#[tokio::test]
async fn send_packet_with_outcome_reports_missing_identity() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let packet = Packet { destination: AddressHash::new_from_rand(OsRng), ..Default::default() };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedMissingDestinationIdentity);
}

#[tokio::test]
async fn send_packet_with_outcome_reports_no_route() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);

    let packet = Packet {
        header: Header { packet_type: PacketType::Data, ..Default::default() },
        context: PacketContext::KeepAlive,
        data: PacketDataBuffer::new_from_slice(&[KEEP_ALIVE_REQUEST]),
        destination: AddressHash::new_from_rand(OsRng),
        ..Default::default()
    };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
}

#[tokio::test]
async fn send_packet_with_outcome_drops_announce_without_route() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);

    let packet = Packet {
        header: Header { packet_type: PacketType::Announce, ..Default::default() },
        destination: AddressHash::new_from_rand(OsRng),
        ..Default::default()
    };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
}

#[tokio::test]
async fn duplicate_filter_allows_repeated_resource_requests() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"same resource request"),
        ..Default::default()
    };

    assert!(handler.lock().await.filter_duplicate_packets(&packet).await);
    assert!(handler.lock().await.filter_duplicate_packets(&packet).await);
}

#[tokio::test]
async fn resource_request_responses_use_bound_link_iface_without_route_lookup() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, false);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager.lock().await.new_channel(8);
    let iface = iface_channel.address;

    let remote_signer = PrivateIdentity::new_from_rand(OsRng);
    let remote_identity = *remote_signer.as_identity();
    let destination = DestinationDesc {
        identity: remote_identity,
        address_hash: remote_identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (link_events, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination, link_events.clone());
    let request_packet = outbound.request();
    let mut inbound = Link::new_from_request(
        &request_packet,
        remote_signer.sign_key().clone(),
        destination,
        link_events,
    )
    .expect("inbound link");
    let proof = inbound.prove();
    let _ = outbound.handle_packet(&proof, iface);
    let link_id = *outbound.id();

    let advertisement_packet = {
        let mut guard = handler.lock().await;
        let link = Arc::new(Mutex::new(outbound));
        guard.out_links.insert(destination.address_hash, link.clone());
        let link_guard = link.lock().await;
        let (resource_hash, packet) = guard
            .resource_manager
            .start_send(&link_guard, vec![0x42; PACKET_MDU + 24], None)
            .expect("start resource");
        guard.resource_manager.confirm_outbound_dispatch(resource_hash, true);
        packet
    };

    let link = handler
        .lock()
        .await
        .out_links
        .get(&destination.address_hash)
        .cloned()
        .expect("outbound link");
    let link_guard = link.lock().await;
    let advertisement = decrypt_resource_advertisement(&link_guard, &advertisement_packet);
    let requested_hashes = advertisement
        .hashmap
        .chunks_exact(MAPHASH_LEN)
        .map(|chunk| {
            let mut hash = [0u8; MAPHASH_LEN];
            hash.copy_from_slice(chunk);
            hash
        })
        .collect::<Vec<_>>();
    let request = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash: advertisement.hash,
        requested_hashes,
    };
    let resource_request_packet = encrypted_resource_control_packet(
        &link_guard,
        PacketContext::ResourceRequest,
        &request.encode(),
    );
    drop(link_guard);

    handle_data(&resource_request_packet, iface, handler.lock().await).await;

    let sent = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
        .await
        .expect("resource parts should be sent on bound iface")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, TxMessageType::Direct(iface));
    assert_eq!(sent.packet.destination, link_id);
    assert_eq!(sent.packet.context, PacketContext::Resource);
}

#[tokio::test]
async fn resource_request_responses_fit_bound_iface_mtu() {
    const LORA_MTU: usize = 220;

    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, false);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut iface_channel = transport.iface_manager.lock().await.new_channel_with_role_mode_mtu(
        8,
        crate::iface::IfaceRole::Unicast,
        crate::iface::InterfaceMode::Full,
        LORA_MTU,
    );
    let iface = iface_channel.address;

    let remote_signer = PrivateIdentity::new_from_rand(OsRng);
    let remote_identity = *remote_signer.as_identity();
    let destination = DestinationDesc {
        identity: remote_identity,
        address_hash: remote_identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (link_events, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination, link_events.clone());
    let request_packet = outbound.request();
    let mut inbound = Link::new_from_request(
        &request_packet,
        remote_signer.sign_key().clone(),
        destination,
        link_events,
    )
    .expect("inbound link");
    let proof = inbound.prove();
    let _ = outbound.handle_packet(&proof, iface);
    let link_id = *outbound.id();

    let advertisement_packet = {
        let mut guard = handler.lock().await;
        let link = Arc::new(Mutex::new(outbound));
        guard.out_links.insert(destination.address_hash, link.clone());
        let link_guard = link.lock().await;
        let (resource_hash, packet) = guard
            .resource_manager
            .start_send_with_mtu(&link_guard, vec![0x42; PACKET_MDU * 2], None, LORA_MTU)
            .expect("start resource");
        guard.resource_manager.confirm_outbound_dispatch(resource_hash, true);
        packet
    };

    let link = handler
        .lock()
        .await
        .out_links
        .get(&destination.address_hash)
        .cloned()
        .expect("outbound link");
    let link_guard = link.lock().await;
    let advertisement = decrypt_resource_advertisement(&link_guard, &advertisement_packet);
    assert!(advertisement.parts > 1, "test payload should require multiple constrained-MTU parts");
    let requested_hashes = advertisement
        .hashmap
        .chunks_exact(MAPHASH_LEN)
        .map(|chunk| {
            let mut hash = [0u8; MAPHASH_LEN];
            hash.copy_from_slice(chunk);
            hash
        })
        .collect::<Vec<_>>();
    let requested_count = requested_hashes.len();
    assert!(requested_count > 0, "advertisement must offer at least one resource part");
    let request = ResourceRequest {
        hashmap_exhausted: false,
        last_map_hash: None,
        resource_hash: advertisement.hash,
        requested_hashes,
    };
    let resource_request_packet = encrypted_resource_control_packet(
        &link_guard,
        PacketContext::ResourceRequest,
        &request.encode(),
    );
    drop(link_guard);

    handle_data(&resource_request_packet, iface, handler.lock().await).await;

    for _ in 0..requested_count {
        let sent = timeout(Duration::from_millis(200), iface_channel.tx_channel.recv())
            .await
            .expect("resource part should be sent on bound iface")
            .expect("tx channel open");
        assert_eq!(sent.tx_type, TxMessageType::Direct(iface));
        assert_eq!(sent.packet.destination, link_id);
        assert_eq!(sent.packet.context, PacketContext::Resource);
        let wire_len = sent.packet.to_bytes().expect("serialize resource part").len();
        assert!(
            wire_len <= LORA_MTU,
            "resource part serialized to {wire_len} bytes, exceeding MTU {LORA_MTU}"
        );
    }
}

fn decrypt_resource_advertisement(link: &Link, packet: &Packet) -> ResourceAdvertisement {
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plain = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .expect("decrypt resource advertisement");
        plain.len()
    };
    buffer.resize(plain_len);
    ResourceAdvertisement::unpack(buffer.as_slice()).expect("resource advertisement")
}
