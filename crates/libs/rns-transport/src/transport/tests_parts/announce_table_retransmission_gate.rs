async fn feed_announce(transport: &Transport, iface: crate::hash::AddressHash, aspect: &str) -> Packet {
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

async fn tier_sizes(transport: &Transport) -> (usize, usize) {
    transport.get_handler().lock().await.announce_table.tier_sizes()
}

/// A node that will never retransmit must not accumulate a retransmission
/// queue. `map` is pruned only by `drain_retransmissions`, and the retransmit
/// worker calls that only when `transport_enabled` — so before this gate,
/// every announce a passive node heard stayed in `map` for the life of the
/// process, one cloned `Packet` per distinct destination.
///
/// The reference does not have the problem because it never inserts:
/// `Transport.py`'s `if (transport_enabled() or is_from_local_client) and
/// context != PATH_RESPONSE:` guards the insert itself.
#[tokio::test]
async fn passive_transport_caches_announces_instead_of_queueing_them() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive", &identity, false));
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    for aspect in ["one", "two", "three"] {
        feed_announce(&transport, iface, aspect).await;
    }

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 0, "a passive node must not queue announces for a retransmission it will never send");
    assert_eq!(cached, 3, "they belong in the bounded cache instead, which is what the path table reads back");
}

/// The control. Same three announces, same code path, `transport_enabled` on:
/// they are queued, because this node really will rebroadcast them. Without
/// this, the assertion above would also pass if `handle_announce` had simply
/// stopped working.
#[tokio::test]
async fn transport_enabled_still_queues_announces_for_retransmission() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("relay", &identity, false);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    for aspect in ["one", "two", "three"] {
        feed_announce(&transport, iface, aspect).await;
    }

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 3, "a transport node must still queue what it is going to rebroadcast");
    assert_eq!(cached, 0, "and must not divert them to the cache");
}

/// Reference parity for the clause this crate models from the other side: an
/// announce arriving over a shared-instance link is queued even on a passive
/// node, matching `or is_from_local_client`.
#[tokio::test]
async fn a_shared_instance_iface_still_queues_on_a_passive_node() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive-shared", &identity, false));
    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let parent = *manager.new_channel(16).address();
        assert!(manager.set_shared_instance(parent, true));
        manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("local client iface")
    };

    feed_announce(&transport, iface, "local-client").await;

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 1, "the reference queues a local client's announce even when not transport-enabled");
    assert_eq!(cached, 0);
}

#[tokio::test]
async fn accepted_announce_fans_out_directly_to_other_local_clients() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("local-client-fanout", &identity, false));
    let (mut host_channel, local_client, other_local_client) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let host_channel = manager.new_channel(16);
        let parent = *host_channel.address();
        assert!(manager.set_shared_instance(parent, true));
        let local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("first local client iface");
        let other_local_client = manager
            .register_virtual_iface(parent, crate::iface::IfaceRole::Unicast)
            .expect("second local client iface");
        (host_channel, local_client, other_local_client)
    };

    let announce = feed_announce(&transport, local_client, "local-client-fanout").await;
    let message = timeout(Duration::from_millis(250), host_channel.tx_channel.recv())
        .await
        .expect("local-client fanout should be immediate")
        .expect("host transmit queue remains open");

    assert!(matches!(
        message.tx_type,
        crate::iface::TxMessageType::Direct(iface) if iface == other_local_client
    ));
    assert_eq!(message.packet.destination, announce.destination);
    assert_eq!(message.packet.data, announce.data);
    assert_eq!(message.packet.context, PacketContext::None);
    assert_eq!(message.packet.transport, Some(*identity.address_hash()));
    assert_eq!(message.packet.header.header_type, crate::packet::HeaderType::Type2);
    assert_eq!(
        message.packet.header.propagation_type,
        crate::packet::PropagationType::Transport
    );
    assert_eq!(message.packet.header.hops, announce.header.hops);
    assert!(timeout(Duration::from_millis(25), host_channel.tx_channel.recv()).await.is_err());
}

/// The third clause of the same reference condition. A path response is a
/// directed reply, not something to rebroadcast, so it is cached rather than
/// queued even on a transport node.
#[tokio::test]
async fn a_path_response_announce_is_never_queued_for_retransmission() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("relay-path-response", &identity, false);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "path-response"),
    );
    let mut announce = destination.announce(OsRng, None).expect("announce");
    announce.context = PacketContext::PathResponse;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let (queued, cached) = tier_sizes(&transport).await;
    assert_eq!(queued, 0, "a path response is a directed reply, not a rebroadcast candidate");
    assert_eq!(cached, 1);
}

/// The reason nothing is simply dropped. This crate rebuilds a path entry's
/// announce packet out of the announce table when persisting the path table,
/// where the reference stores a packet hash and keeps the packet in its own
/// on-disk cache. A passive node that dropped the packet here would persist an
/// empty path table — so the cached copy has to remain findable.
#[tokio::test]
async fn a_cached_announce_is_still_findable_for_path_table_persistence() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("passive-persist", &identity, false));
    let iface = transport.iface_manager().lock().await.new_channel(16).address;

    let announce = feed_announce(&transport, iface, "persisted").await;

    let handler_arc = transport.get_handler();
    let handler = handler_arc.lock().await;
    let found = handler.announce_table.cached_packet_for_destination(&announce.destination);
    assert!(
        found.is_some(),
        "save_reticulum_path_table drops any path entry whose announce packet it cannot find"
    );
}
