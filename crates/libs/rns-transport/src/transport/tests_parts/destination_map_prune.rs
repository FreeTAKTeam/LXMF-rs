#[tokio::test]
async fn destination_cleanup_prunes_expired_routes_but_keeps_live_references() {
    let local = PrivateIdentity::new_from_name("cache-cleanup-local");
    let transport = Transport::new(TransportConfig::new("cleanup", &local, true));
    let iface = *transport.iface_manager().lock().await.new_channel(32).address();
    let name = DestinationName::new("lxmf", "delivery");
    let local_destination = transport.add_destination(local, name).await;
    let local_hash = local_destination.lock().await.desc.address_hash;
    let expired = learn_cached_path(&transport, iface, "cleanup-expired").await;
    let live = learn_cached_path(&transport, iface, "cleanup-live").await;
    let linked = learn_cached_path(&transport, iface, "cleanup-linked").await;
    let linked_desc = transport.get_handler().lock().await.single_out_destinations
        .get(&linked.destination).expect("linked recipient").lock().await.desc;
    let linked_hash = linked_desc.address_hash;
    transport.link(linked_desc).await;
    let prepared_identity = PrivateIdentity::new_from_name("cleanup-prepared");
    let prepared = SingleOutputDestination::new(*prepared_identity.as_identity(), name);
    let prepared_hash = prepared.desc.address_hash;
    transport.send_prepared_single_packet_observed_with_trace(
        Packet { destination: prepared_hash, data: PacketDataBuffer::new_from_slice(b"pending proof"), ..Packet::default() },
        *prepared_identity.as_identity(), name, |_| {},
    ).await.expect("prepared send");
    let expired_hash = expired.destination;
    let live_hash = live.destination;
    let handler = transport.get_handler();
    {
        let mut handler = handler.lock().await;
        handler.path_table.expire_path(&expired_hash);
        handler.path_table.expire_path(&linked_hash);
        let local_identity = *handler.config.identity.as_identity();
        handler.single_out_destinations.insert(local_hash, Arc::new(Mutex::new(
            SingleOutputDestination::new(local_identity, name)
        )));
        handler.prune_destination_cache().await;
        assert!(!handler.single_out_destinations.contains_key(&expired_hash));
        for address in [live_hash, local_hash, linked_hash, prepared_hash] {
            assert!(handler.single_out_destinations.contains_key(&address), "live reference {address}");
        }
        assert_eq!(handler.single_out_destinations_removed_total, 1);
        handler.packet_cache.lock().await.release(std::time::Duration::ZERO);
        handler.prune_destination_cache().await;
        assert!(!handler.single_out_destinations.contains_key(&prepared_hash), "expired receipt releases recipient");
    }
    assert_eq!(transport.destination_is_cached_now(&expired_hash), Some(false));
    let guard = handler.lock().await;
    assert_eq!(transport.destination_is_cached_now(&live_hash), None, "busy means retain");
    drop(guard);
    assert_eq!(transport.destination_is_cached_now(&live_hash), Some(true));
}

#[tokio::test]
async fn destination_cleanup_preserves_identified_inbound_backchannel() {
    let local = PrivateIdentity::new_from_name("backchannel-cleanup");
    let transport = Transport::new(TransportConfig::new("backchannel-cleanup", &local, true));
    let local_destination = SingleInputDestination::new(local.clone(), DestinationName::new("lxmf", "delivery"));
    let (link, peer_destination) = identified_inbound_link(&transport, &local, local_destination.desc).await;
    let identity = *link.lock().await.identified_peer_identity().expect("identified remote");
    let output = SingleOutputDestination::new(identity, local_destination.desc.name);
    assert_eq!(output.desc.address_hash, peer_destination);
    let handler = transport.get_handler();
    let mut handler = handler.lock().await;
    assert!(handler.path_table.get(&peer_destination).is_none());
    handler.single_out_destinations.insert(peer_destination, Arc::new(Mutex::new(output)));
    handler.prune_destination_cache().await;
    assert!(handler.single_out_destinations.contains_key(&peer_destination));
    handler.in_links.clear();
    handler.prune_destination_cache().await;
    assert!(!handler.single_out_destinations.contains_key(&peer_destination));
}
