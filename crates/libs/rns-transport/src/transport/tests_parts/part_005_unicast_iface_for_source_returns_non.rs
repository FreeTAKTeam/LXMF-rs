#[tokio::test]
async fn unicast_iface_for_source_returns_none_for_non_multicast_iface() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let unicast_iface = new_unicast_iface_in(&transport).await;
    let handler = transport.get_handler();

    let result = handler
        .lock()
        .await
        .unicast_iface_for_source(unicast_iface, crate::iface::IfaceSource::Udp(peer_addr(4242)))
        .await;

    assert_eq!(result, None, "non-multicast iface must not trigger auto-unicast");
}

#[tokio::test]
async fn unicast_iface_for_source_returns_none_when_source_is_not_udp() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let result = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::None)
        .await;

    assert_eq!(result, None, "no source addr means no auto-unicast");
}

#[tokio::test]
async fn unicast_iface_for_source_returns_none_when_no_peer_routing_registered() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    // Register a Multicast-tagged iface *without* a PeerRouting map.
    let mc_iface = {
        let mgr = transport.iface_manager();
        let mut mgr = mgr.lock().await;
        let channel = mgr.new_channel_with_role(16, crate::iface::IfaceRole::Multicast);
        *channel.address()
    };
    let handler = transport.get_handler();

    let result = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer_addr(4242)))
        .await;

    assert_eq!(
        result, None,
        "missing PeerRouting means we can't register — bail rather than silently misroute"
    );
}

#[tokio::test]
async fn unicast_iface_for_source_registers_virtual_iface_and_peer_routing() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let iface_count_before = { transport.iface_manager().lock().await.iface_count() };

    let peer = peer_addr(4242);
    let virtual_hash = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("should register a virtual iface");

    assert_ne!(
        virtual_hash, mc_iface,
        "virtual iface hash is distinct from the host multicast iface"
    );

    // A single LocalInterface entry was added (the virtual one).
    let iface_count_after = { transport.iface_manager().lock().await.iface_count() };
    assert_eq!(iface_count_after, iface_count_before + 1);

    // Role is VirtualUnicast so InterfaceManager::send skips it on Broadcast tx.
    let role = { transport.iface_manager().lock().await.role(&virtual_hash) };
    assert_eq!(role, Some(crate::iface::IfaceRole::VirtualUnicast));

    // Handler tracks it.
    let guard = handler.lock().await;
    assert_eq!(guard.unicast_udp_ifaces.len(), 1);
    assert_eq!(guard.unicast_udp_ifaces.get(&peer).map(|(h, _)| *h), Some(virtual_hash),);

    // And the PeerRouting map has the forward + reverse entries.
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing");
    let routing = routing.lock().await;
    assert_eq!(routing.hash_for_addr(&peer), Some(virtual_hash));
    assert_eq!(routing.addr_for_hash(&virtual_hash), Some(peer));
}

#[tokio::test]
async fn unicast_iface_for_source_reuses_existing_virtual_iface_for_same_peer() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer = peer_addr(4242);
    let first = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("first");
    let second = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("second");

    assert_eq!(first, second, "same peer reuses the same virtual iface hash");

    let guard = handler.lock().await;
    assert_eq!(guard.unicast_udp_ifaces.len(), 1);
}

#[tokio::test]
async fn unicast_iface_for_source_registers_distinct_virtual_ifaces_for_distinct_peers() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer_a = peer_addr(4242);
    let peer_b = peer_addr(5252);

    let iface_a = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer_a))
        .await
        .expect("peer a");
    let iface_b = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer_b))
        .await
        .expect("peer b");

    assert_ne!(iface_a, iface_b);

    let guard = handler.lock().await;
    assert_eq!(guard.unicast_udp_ifaces.len(), 2);
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer_a), Some(iface_a));
    assert_eq!(routing.hash_for_addr(&peer_b), Some(iface_b));
}

#[tokio::test]
async fn unicast_iface_for_source_refreshes_last_seen_on_repeat_call() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer = peer_addr(4242);
    handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("register");

    {
        let mut guard = handler.lock().await;
        let entry = guard.unicast_udp_ifaces.get_mut(&peer).expect("cached");
        entry.1 = tokio::time::Instant::now() - Duration::from_secs(600);
    }

    handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("refresh");

    let guard = handler.lock().await;
    let (_, last_seen) = guard.unicast_udp_ifaces.get(&peer).expect("cached");
    let age = tokio::time::Instant::now().saturating_duration_since(*last_seen);
    assert!(age < Duration::from_secs(1), "last_seen must be refreshed; got age {:?}", age,);
}

#[tokio::test]
async fn gc_unicast_ifaces_removes_stale_entries_from_routing_and_manager() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let stale_peer = peer_addr(4242);
    let fresh_peer = peer_addr(5252);

    let stale_iface = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(stale_peer))
        .await
        .expect("stale");
    let fresh_iface = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(fresh_peer))
        .await
        .expect("fresh");

    {
        let mut guard = handler.lock().await;
        let entry = guard.unicast_udp_ifaces.get_mut(&stale_peer).expect("cached");
        entry.1 = tokio::time::Instant::now() - Duration::from_secs(3600);
    }

    handler.lock().await.gc_unicast_ifaces().await;

    let guard = handler.lock().await;
    assert!(!guard.unicast_udp_ifaces.contains_key(&stale_peer));
    assert!(guard.unicast_udp_ifaces.contains_key(&fresh_peer));

    // PeerRouting map no longer contains the stale peer.
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&stale_peer), None);
    assert_eq!(routing.hash_for_addr(&fresh_peer), Some(fresh_iface));
    assert_eq!(routing.addr_for_hash(&stale_iface), None);
    drop(routing);

    // InterfaceManager stopped the stale virtual iface (role lookup now None).
    let mgr = transport.iface_manager();
    let mgr = mgr.lock().await;
    assert_eq!(mgr.role(&stale_iface), None);
    assert_eq!(mgr.role(&fresh_iface), Some(crate::iface::IfaceRole::VirtualUnicast));
}

#[tokio::test]
async fn gc_unicast_ifaces_is_noop_when_no_entries_are_stale() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer = peer_addr(4242);
    let iface = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("register");

    handler.lock().await.gc_unicast_ifaces().await;

    let guard = handler.lock().await;
    assert!(guard.unicast_udp_ifaces.contains_key(&peer));
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer), Some(iface));
}
