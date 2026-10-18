async fn pending_proof_fixture(
    hops: Option<u8>,
    attached: bool,
) -> (Transport, Arc<Mutex<Link>>, Link, crate::iface::InterfaceChannel, ed25519_dalek::SigningKey)
{
    let config = TransportConfig {
        broadcast: true,
        connected_to_shared_instance: attached,
        ..TransportConfig::default()
    };
    let transport = Transport::new(config);
    let mut channel = transport.iface_manager().lock().await.new_channel(16);
    let destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "proof-hop-selection"),
    );
    if attached {
        transport.iface_manager().lock().await.set_shared_instance(*channel.address(), true);
    }
    if let Some(hops) = hops {
        transport.handler.lock().await.path_table.restore_tunnel_path(
            destination.desc.address_hash,
            destination.desc.address_hash,
            hops,
            *channel.address(),
            Hash::new([43; HASH_SIZE]),
            std::time::Instant::now(),
        );
    }
    let link = transport.link(destination.desc).await;
    transport.cancel.cancel();
    let request = timeout(Duration::from_secs(1), channel.tx_channel.recv())
        .await
        .expect("initial native request handoff")
        .expect("request")
        .packet;
    let signing_key = destination.sign_key().clone();
    let peer = Link::new_from_request(
        &request,
        destination.sign_key().clone(),
        destination.desc,
        tokio::sync::broadcast::channel(8).0,
    )
    .expect("responder binds native request");
    (transport, link, peer, channel, signing_key)
}

#[tokio::test]
async fn pending_proof_equal_hop_invalid_is_sticky_but_mismatch_invalid_is_not() {
    for attached in [false, true] {
        let (transport, link, mut peer, channel, _) =
            pending_proof_fixture(Some(2), attached).await;
        let cache = transport.handler.lock().await.packet_cache.clone();
        let mut invalid = peer.prove();
        invalid.data.as_mut_slice()[0] ^= 1;
        // Normal ingress increments; a configured shared boundary retains wire hops.
        invalid.header.hops = if attached { 2 } else { 1 };
        commit_local_link_packet(&transport, invalid.clone(), *channel.address()).await;
        assert_eq!(link.lock().await.status(), LinkStatus::Pending);
        assert_eq!(
            cache.lock().await.contains_hash(&invalid.hash()),
            !attached,
            "equal-hop selection remembers before signature validation"
        );

        let mut mismatch = invalid;
        mismatch.data.as_mut_slice()[1] ^= 1;
        mismatch.header.hops = if attached { 3 } else { 2 };
        commit_local_link_packet(&transport, mismatch.clone(), *channel.address()).await;
        assert_eq!(link.lock().await.status(), LinkStatus::Pending);
        assert!(
            !cache.lock().await.contains_hash(&mismatch.hash()),
            "invalid mismatch cannot authenticate hop rebalancing"
        );
    }
}

#[tokio::test]
async fn pending_proof_signed_mismatch_rebalances_existing_path_and_activates() {
    let (transport, link, mut peer, channel, _) = pending_proof_fixture(Some(2), false).await;
    let destination = link.lock().await.destination().address_hash;
    let before = {
        let handler = transport.handler.lock().await;
        let entry = handler.path_table.get(&destination).expect("existing path");
        (entry.timestamp, entry.received_from, entry.iface, entry.packet_hash)
    };
    let mut proof = peer.prove();
    proof.header.hops = 3;
    commit_local_link_packet(&transport, proof.clone(), *channel.address()).await;
    assert_eq!(link.lock().await.status(), LinkStatus::Active);
    let handler = transport.handler.lock().await;
    let entry = handler.path_table.get(&destination).expect("path survives rebalancing");
    assert_eq!(entry.hops, 4);
    assert_eq!(
        (entry.timestamp, entry.received_from, entry.iface, entry.packet_hash),
        before,
        "rebalancing changes only the existing hop field"
    );
    assert!(handler.packet_cache.lock().await.contains_hash(&proof.hash()));
}

#[tokio::test]
async fn pending_proof_dispatch_binds_unknown_and_shared_zero_one_without_adjustment() {
    for (hops, attached, expected) in [(None, false, 128), (Some(0), true, 0), (Some(1), true, 1)] {
        let (transport, link, _, _, _) = pending_proof_fixture(hops, attached).await;
        assert_eq!(link.lock().await.expected_proof_hops(), Some(expected));
        let destination = link.lock().await.destination().address_hash;
        assert_eq!(
            transport.handler.lock().await.path_table.get(&destination).is_some(),
            hops.is_some()
        );
    }
    // Even a deliberately dropped no-route request binds its generation.
    let (transport, link, _, _, _) = pending_proof_fixture(None, false).await;
    let destination = link.lock().await.destination().address_hash;
    link.lock().await.close();
    let request = link.lock().await.request();
    transport.handler.lock().await.config.broadcast = false;
    let trace = transport.handler.lock().await.send_packet_with_trace(request).await;
    assert_eq!(trace.outcome, SendPacketOutcome::DroppedNoRoute);
    assert_eq!(link.lock().await.expected_proof_hops(), Some(128));
    assert!(transport.handler.lock().await.path_table.get(&destination).is_none());
}

#[tokio::test]
async fn pending_proof_dispatch_uses_current_route_and_retry_preserves_generation_binding() {
    let (transport, link, mut peer, mut channel, _) = pending_proof_fixture(Some(3), false).await;
    let original_id = *link.lock().await.id();
    let destination = link.lock().await.destination().address_hash;
    let retry = link.lock().await.request();
    let delayed_old_request = retry.clone();
    {
        let mut handler = transport.handler.lock().await;
        handler.path_table.rebalance_hops(&destination, 1);
        handler.send_packet(retry).await;
    }
    let _ = channel.tx_channel.recv().await.expect("retry handed off");
    assert_eq!(*link.lock().await.id(), original_id);
    assert_eq!(link.lock().await.expected_proof_hops(), Some(3), "retry cannot rebind");

    link.lock().await.close();
    let restarted = link.lock().await.request();
    let new_id = *link.lock().await.id();
    assert_ne!(new_id, original_id);
    assert_eq!(link.lock().await.expected_proof_hops(), None);
    {
        let mut handler = transport.handler.lock().await;
        handler.send_packet(delayed_old_request).await;
        assert_eq!(
            link.lock().await.expected_proof_hops(),
            None,
            "delayed old dispatch cannot bind the restarted generation"
        );
        // The route changes after request construction, before actual dispatch.
        handler.path_table.rebalance_hops(&destination, 2);
        handler.send_packet(restarted).await;
    }
    let _ = channel.tx_channel.recv().await.expect("delayed old request handed off");
    let _ = channel.tx_channel.recv().await.expect("restarted request handed off");
    assert_eq!(link.lock().await.expected_proof_hops(), Some(2));
    let old_proof = peer.prove();
    commit_local_link_packet(&transport, old_proof.clone(), *channel.address()).await;
    assert_eq!(link.lock().await.status(), LinkStatus::Pending);
    assert!(!transport
        .handler
        .lock()
        .await
        .packet_cache
        .lock()
        .await
        .contains_hash(&old_proof.hash()));
    assert_eq!(link.lock().await.expected_proof_hops(), Some(2));
}

#[tokio::test]
async fn pending_proof_queued_before_restart_or_replacement_cannot_mutate_new_generation() {
    for replace in [false, true] {
        let (transport, previous, mut peer, channel, _) =
            pending_proof_fixture(Some(2), false).await;
        let destination = *previous.lock().await.destination();
        let mut proof = peer.prove();
        proof.header.hops = 3;
        let (class, queued) = preprocess_inbound_message(
            &transport.handler,
            &transport.iface_messages_tx,
            RxMessage {
                address: *channel.address(),
                packet: proof.clone(),
                source: IfaceSource::None,
            },
        )
        .await
        .expect("old proof queued before generation change");
        let queues = InboundQueues::new(InboundQueueLimits::default());
        assert!(
            super::inbound_processing::enqueue_inbound_message(
                &transport.handler,
                &queues,
                class,
                queued,
            )
            .await
        );
        previous.lock().await.close();
        let current = if replace {
            transport.link(destination).await
        } else {
            let request = previous.lock().await.request();
            transport.handler.lock().await.send_packet(request).await;
            previous.clone()
        };
        super::inbound_processing::process_inbound_message(
            transport.handler.clone(),
            queues.try_dequeue().expect("queued proof drains after change"),
        )
        .await;
        assert_eq!(current.lock().await.status(), LinkStatus::Pending);
        assert_eq!(current.lock().await.expected_proof_hops(), Some(2));
        let handler = transport.handler.lock().await;
        assert_eq!(handler.path_table.hops_to(&destination.address_hash), 2);
        assert!(!handler.packet_cache.lock().await.contains_hash(&proof.hash()));
    }
}

#[tokio::test]
async fn explicit_restart_before_new_request_rejects_queued_old_proof_then_new_generation_activates(
) {
    let (transport, link, mut peer, mut channel, signing_key) =
        pending_proof_fixture(Some(2), false).await;
    let old_request = link.lock().await.request();
    let old_id = *link.lock().await.id();
    let destination = *link.lock().await.destination();
    let mut old_proof = peer.prove();
    old_proof.header.hops = 3;
    let queues = InboundQueues::new(InboundQueueLimits::default());
    let (class, queued) = preprocess_inbound_message(
        &transport.handler,
        &transport.iface_messages_tx,
        RxMessage {
            address: *channel.address(),
            packet: old_proof.clone(),
            source: IfaceSource::None,
        },
    )
    .await
    .expect("previous generation proof not yet ingressed");
    assert!(
        super::inbound_processing::enqueue_inbound_message(
            &transport.handler,
            &queues,
            class,
            queued
        )
        .await
    );
    link.lock().await.restart();
    assert_eq!(*link.lock().await.id(), old_id);
    transport.handler.lock().await.send_packet(old_request).await;
    channel.tx_channel.try_recv().expect("delayed old frame dispatched");
    assert_eq!(link.lock().await.expected_proof_hops(), None);
    super::inbound_processing::process_inbound_message(
        transport.handler.clone(),
        queues.try_dequeue().expect("drain during restart before request"),
    )
    .await;
    assert_eq!(link.lock().await.status(), LinkStatus::Pending);
    assert_eq!(transport.handler.lock().await.path_table.hops_to(&destination.address_hash), 2);
    assert!(!transport
        .handler
        .lock()
        .await
        .packet_cache
        .lock()
        .await
        .contains_hash(&old_proof.hash()));
    assert!(channel.tx_channel.try_recv().is_err(), "old proof cannot activate or send RTT");
    let request = link.lock().await.request();
    let new_id = *link.lock().await.id();
    assert_ne!(new_id, old_id);
    {
        let mut handler = transport.handler.lock().await;
        handler.path_table.rebalance_hops(&destination.address_hash, 1);
        handler.send_packet(request).await;
    }
    assert_eq!(link.lock().await.expected_proof_hops(), Some(1));
    let request = channel.tx_channel.try_recv().expect("new generation dispatch").packet;
    let mut peer = Link::new_from_request(
        &request,
        signing_key,
        destination,
        tokio::sync::broadcast::channel(4).0,
    )
    .expect("new responder generation");
    commit_local_link_packet(&transport, peer.prove(), *channel.address()).await;
    assert_eq!(link.lock().await.status(), LinkStatus::Active);
    assert_eq!(*link.lock().await.id(), new_id);
    assert_eq!(
        channel.tx_channel.try_recv().expect("new generation RTT").packet.context,
        PacketContext::LinkRTT
    );
}
