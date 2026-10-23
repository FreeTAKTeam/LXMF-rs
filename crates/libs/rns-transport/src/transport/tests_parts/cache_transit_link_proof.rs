#[tokio::test]
async fn repeated_transit_link_request_proofs_forward_without_local_membership() {
    let mut config = TransportConfig::default();
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let (mut requester, responder, wrong) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(8), manager.new_channel(8), manager.new_channel(8))
    };
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let name = DestinationName::new("lxmf", "delivery");
    let destination = SingleInputDestination::new(identity, name);
    let (events, _) = tokio::sync::broadcast::channel(4);
    let mut initiator = Link::new(destination.desc, events.clone());
    let request = initiator.request();
    let mut peer =
        Link::new_from_request(&request, destination.sign_key().clone(), destination.desc, events)
            .expect("native request binds proof signer and Link id");
    let proof = peer.prove();
    {
        let mut handler = transport.handler.lock().await;
        handler.single_out_destinations.insert(
            destination.desc.address_hash,
            Arc::new(Mutex::new(SingleOutputDestination::new(destination.desc.identity, name))),
        );
        handler.link_table.add(
            &request,
            request.destination,
            *requester.address(),
            destination.desc.address_hash,
            *responder.address(),
            0,
        );
    }
    let cache = transport.handler.lock().await.packet_cache.clone();
    commit_local_link_packet(&transport, proof.clone(), *wrong.address()).await;
    assert!(requester.tx_channel.try_recv().is_err());
    assert!(!cache.lock().await.contains_hash(&proof.hash()));
    let mut invalid = proof.clone();
    invalid.data.as_mut_slice()[0] ^= 1;
    commit_local_link_packet(&transport, invalid.clone(), *responder.address()).await;
    assert!(requester.tx_channel.try_recv().is_err());
    assert!(!cache.lock().await.contains_hash(&invalid.hash()));
    for replay in 0..2 {
        commit_local_link_packet(&transport, proof.clone(), *responder.address()).await;
        let forwarded = timeout(Duration::from_secs(1), requester.tx_channel.recv())
            .await
            .expect("transit proof handoff deadline")
            .expect("transit proof");
        assert_eq!(forwarded.packet.hash(), proof.hash());
        assert!(
            !cache.lock().await.contains_hash(&proof.hash()),
            "transit LRPROOF replay {replay} must not be remembered by generic outbound dispatch"
        );
        assert!(requester.tx_channel.try_recv().is_err());
    }
    transport.cancel.cancel();
}

#[tokio::test]
async fn transit_keepalive_must_match_validated_direction_and_hops_before_remembering() {
    let mut config = TransportConfig::default();
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    transport.cancel.cancel();
    let (mut requester, mut responder, wrong) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(8), manager.new_channel(8), manager.new_channel(8))
    };
    let destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "keepalive-relay"),
    );
    let events = tokio::sync::broadcast::channel(4).0;
    let mut local = Link::new(destination.desc, events.clone());
    let mut request = local.request();
    request.header.hops = 2;
    let mut peer =
        Link::new_from_request(&request, destination.sign_key().clone(), destination.desc, events)
            .expect("responder");
    {
        let mut handler = transport.handler.lock().await;
        handler.link_table.add(
            &request,
            request.destination,
            *requester.address(),
            destination.desc.address_hash,
            *responder.address(),
            0,
        );
    }
    let mut response = peer.prove();
    response.header.packet_type = PacketType::Data;
    response.context = PacketContext::KeepAlive;
    response.data = PacketDataBuffer::new_from_slice(&[KEEP_ALIVE_RESPONSE]);
    response.header.hops = 3;
    let cache = transport.handler.lock().await.packet_cache.clone();
    commit_local_link_packet(&transport, response.clone(), *wrong.address()).await;
    assert!(
        requester.tx_channel.try_recv().is_err(),
        "unvalidated/wrong-interface keepalive cannot forward"
    );
    assert!(!cache.lock().await.contains_hash(&response.hash()));
    {
        let mut handler = transport.handler.lock().await;
        let mut proof = peer.prove();
        proof.header.hops = 4;
        assert!(handler.link_table.handle_proof(&proof).is_some());
    }
    response.header.hops = 4; // processed hops5 differs from remaining4
    commit_local_link_packet(&transport, response.clone(), *responder.address()).await;
    assert!(
        requester.tx_channel.try_recv().is_err(),
        "wrong-hop keepalive cannot bypass common gate"
    );
    assert!(!cache.lock().await.contains_hash(&response.hash()));
    response.header.hops = 3;
    commit_local_link_packet(&transport, response.clone(), *responder.address()).await;
    assert_eq!(
        requester.tx_channel.try_recv().expect("valid responder direction").packet.header.hops,
        4
    );
    response.header.hops = 1;
    commit_local_link_packet(&transport, response.clone(), *requester.address()).await;
    assert_eq!(
        responder.tx_channel.try_recv().expect("valid requester direction").packet.header.hops,
        2
    );
}

#[tokio::test]
async fn transit_request_captures_route_and_only_first_valid_proof_can_rebalance_it() {
    let mut config = TransportConfig::default();
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    transport.cancel.cancel();
    let (mut requester, mut responder) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(8), manager.new_channel(8))
    };
    let name = DestinationName::new("lxmf", "transit-hops");
    let destination = SingleInputDestination::new(PrivateIdentity::new_from_rand(OsRng), name);
    let events = tokio::sync::broadcast::channel(4).0;
    let mut local = Link::new(destination.desc, events.clone());
    let request = local.request();
    let id = LinkId::from(&request);
    {
        let mut handler = transport.handler.lock().await;
        handler.single_out_destinations.insert(
            destination.desc.address_hash,
            Arc::new(Mutex::new(SingleOutputDestination::new(destination.desc.identity, name))),
        );
        handler.path_table.restore_tunnel_path(
            destination.desc.address_hash,
            destination.desc.address_hash,
            3,
            *responder.address(),
            Hash::new([19; HASH_SIZE]),
            std::time::Instant::now(),
        );
    }
    commit_local_link_packet(&transport, request, *requester.address()).await;
    let request = responder.tx_channel.try_recv().expect("real routed native request").packet;
    assert_eq!(transport.handler.lock().await.link_table.remaining_hops(&id), Some(3));
    let mut peer =
        Link::new_from_request(&request, destination.sign_key().clone(), destination.desc, events)
            .expect("responder accepts routed request");
    let mut proof = peer.prove();
    proof.header.hops = 1;
    let mut invalid = proof.clone();
    invalid.data.as_mut_slice()[0] ^= 1;
    commit_local_link_packet(&transport, invalid.clone(), *responder.address()).await;
    assert!(requester.tx_channel.try_recv().is_err());
    {
        let handler = transport.handler.lock().await;
        assert_eq!(handler.link_table.remaining_hops(&id), Some(3));
        assert_eq!(handler.path_table.hops_to(&destination.desc.address_hash), 3);
        assert!(!handler.packet_cache.lock().await.contains_hash(&invalid.hash()));
    }
    commit_local_link_packet(&transport, proof.clone(), *responder.address()).await;
    requester.tx_channel.try_recv().expect("valid first proof forwards");
    {
        let handler = transport.handler.lock().await;
        assert_eq!(handler.link_table.remaining_hops(&id), Some(2));
        assert_eq!(handler.path_table.hops_to(&destination.desc.address_hash), 2);
    }
    proof.header.hops = 3;
    commit_local_link_packet(&transport, proof.clone(), *responder.address()).await;
    assert!(requester.tx_channel.try_recv().is_err(), "validated transit distance cannot rebind");
    assert_eq!(transport.handler.lock().await.link_table.remaining_hops(&id), Some(2));
    assert_eq!(
        transport.handler.lock().await.path_table.hops_to(&destination.desc.address_hash),
        2
    );
    assert!(!transport.handler.lock().await.packet_cache.lock().await.contains_hash(&proof.hash()));
}

#[tokio::test]
async fn queued_link_data_uses_current_table_insertion_expiry_and_proof_activation() {
    for change in ["insert_pending", "expire_pending", "validate_pending"] {
        let mut config = TransportConfig::default();
        config.set_transport_enabled(true);
        let transport = Transport::new(config);
        transport.cancel.cancel();
        let (mut requester, responder) = {
            let manager = transport.iface_manager();
            let mut manager = manager.lock().await;
            (manager.new_channel(8), manager.new_channel(8))
        };
        let name = DestinationName::new("lxmf", "queued-transit");
        let destination = SingleInputDestination::new(PrivateIdentity::new_from_rand(OsRng), name);
        let events = tokio::sync::broadcast::channel(4).0;
        let mut local = Link::new(destination.desc, events.clone());
        let mut request = local.request();
        request.header.hops = 1;
        let mut peer = Link::new_from_request(
            &request,
            destination.sign_key().clone(),
            destination.desc,
            events,
        )
        .expect("native peer");
        let mut proof = peer.prove();
        proof.header.hops = 1;
        let mut data = peer
            .data_packet(b"opaque authenticated transit payload")
            .expect("native encrypted data");
        data.header.hops = 1;
        {
            let mut handler = transport.handler.lock().await;
            handler.single_out_destinations.insert(
                destination.desc.address_hash,
                Arc::new(Mutex::new(SingleOutputDestination::new(destination.desc.identity, name))),
            );
            handler.link_table = LinkTable::new(Duration::ZERO, Duration::from_secs(30));
            if change != "insert_pending" {
                handler.link_table.add(
                    &request,
                    request.destination,
                    *requester.address(),
                    destination.desc.address_hash,
                    *responder.address(),
                    2,
                );
            }
        }
        let queues = InboundQueues::new(InboundQueueLimits::default());
        let (class, queued) = preprocess_inbound_message(
            &transport.handler,
            &transport.iface_messages_tx,
            RxMessage {
                address: *responder.address(),
                packet: data.clone(),
                source: IfaceSource::None,
            },
        )
        .await
        .expect("Link Data candidate");
        assert!(
            super::inbound_processing::enqueue_inbound_message(
                &transport.handler,
                &queues,
                class,
                queued
            )
            .await
        );
        match change {
            "insert_pending" => transport.handler.lock().await.link_table.add(
                &request,
                request.destination,
                *requester.address(),
                destination.desc.address_hash,
                *responder.address(),
                2,
            ),
            "expire_pending" => {
                assert_eq!(transport.handler.lock().await.link_table.remove_stale().len(), 1)
            }
            "validate_pending" => {
                commit_local_link_packet(&transport, proof, *responder.address()).await;
                requester.tx_channel.try_recv().expect("real proof forwarded before queued Data");
            }
            _ => unreachable!("declared table transition"),
        }
        super::inbound_processing::process_inbound_message(
            transport.handler.clone(),
            queues.try_dequeue().expect("drain after native table transition"),
        )
        .await;
        let forwarded = requester.tx_channel.try_recv().ok();
        assert_eq!(forwarded.is_some(), change == "validate_pending", "transition {change}");
        if let Some(message) = forwarded {
            assert_eq!(message.packet.hash(), data.hash());
        }
        assert_eq!(
            transport.handler.lock().await.packet_cache.lock().await.contains_hash(&data.hash()),
            change != "insert_pending",
            "transition {change} decides current remembering owner"
        );
    }
}
