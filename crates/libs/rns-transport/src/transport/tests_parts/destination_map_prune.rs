use super::*;

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
    let linked_desc = transport
        .get_handler()
        .lock()
        .await
        .single_out_destinations
        .get(&linked.destination)
        .expect("linked recipient")
        .lock()
        .await
        .desc;
    let linked_hash = linked_desc.address_hash;
    transport.link(linked_desc).await;
    let prepared_identity = PrivateIdentity::new_from_name("cleanup-prepared");
    let prepared = SingleOutputDestination::new(*prepared_identity.as_identity(), name);
    let prepared_hash = prepared.desc.address_hash;
    transport
        .send_prepared_single_packet_observed_with_trace(
            Packet {
                destination: prepared_hash,
                data: PacketDataBuffer::new_from_slice(b"pending proof"),
                ..Packet::default()
            },
            *prepared_identity.as_identity(),
            name,
            |_| {},
        )
        .await
        .expect("prepared send");
    let expired_hash = expired.destination;
    let live_hash = live.destination;
    let handler = transport.get_handler();
    {
        let mut handler = handler.lock().await;
        handler.path_table.expire_path(&expired_hash);
        handler.path_table.expire_path(&linked_hash);
        let local_identity = *handler.config.identity.as_identity();
        handler.single_out_destinations.insert(
            local_hash,
            Arc::new(Mutex::new(SingleOutputDestination::new(local_identity, name))),
        );
        handler.prune_destination_cache().await;
        assert!(!handler.single_out_destinations.contains_key(&expired_hash));
        for address in [live_hash, local_hash, linked_hash, prepared_hash] {
            assert!(
                handler.single_out_destinations.contains_key(&address),
                "live reference {address}"
            );
        }
        assert_eq!(handler.single_out_destinations_removed_total, 1);
        handler.packet_cache.lock().await.release(std::time::Duration::ZERO);
        handler.prune_destination_cache().await;
        assert!(
            !handler.single_out_destinations.contains_key(&prepared_hash),
            "expired receipt releases recipient"
        );
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
    let local_destination =
        SingleInputDestination::new(local.clone(), DestinationName::new("lxmf", "delivery"));
    let (link, peer_destination) =
        identified_inbound_link(&transport, &local, local_destination.desc).await;
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

#[tokio::test]
async fn restored_tunnel_identity_survives_cleanup_until_last_tunnel_expires() {
    use crate::transport::tunnels::{PythonTunnelEntry, PythonTunnelPathEntry};

    let storage = tempfile::tempdir().expect("tunnel identity storage");
    let local = PrivateIdentity::new_from_name("tunnel-identity-cleanup");
    let transport = Transport::new(TransportConfig::new("before", &local, true));
    let iface = *transport.iface_manager().lock().await.new_channel(32).address();
    let learned = learn_cached_path(&transport, iface, "delivery").await;
    let expected = transport.destination_identity(&learned.destination).await.expect("learned key");
    let now = std::time::Instant::now();
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs_f64();
    let tunnel_ids = [Hash::new_from_slice(&[1; 32]), Hash::new_from_slice(&[2; 32])];
    let tunnels = tunnel_ids
        .iter()
        .map(|tunnel_id| PythonTunnelEntry {
            tunnel_id: *tunnel_id,
            interface_hash: None,
            expires_secs: unix + 3600.0,
            paths: vec![PythonTunnelPathEntry {
                destination: learned.destination,
                timestamp_secs: unix,
                received_from: learned.destination,
                hops: 1,
                expires_secs: unix + 3600.0,
                random_blobs: vec![],
                interface_hash: None,
                packet_hash: learned.packet.hash(),
            }],
        })
        .collect();
    transport.get_handler().lock().await.tunnel_table.restore_python_entries(tunnels, now, unix);
    transport.save_reticulum_path_table(storage.path()).await.expect("persist valid announce");

    // No interface is registered in the replacement transport. The active row
    // cannot map, but the validated cached announce restores the detached key.
    let restored = Transport::new(TransportConfig::new("after", &local, true));
    let report =
        restored.restore_reticulum_path_table_report(storage.path()).await.expect("restore");
    assert_eq!(report.restored_active_paths, 0);
    assert_eq!(report.skipped.active_unmapped_interface, 1);
    assert_eq!(report.restored_identities.len(), 1);
    assert_eq!(report.skipped.tunnel_duplicate_packet_hash, 1);
    assert_eq!(restored.table_occupancy().await.tunnel_path_entries, 2);
    assert!(!restored.has_path(&learned.destination).await);
    let handler = restored.get_handler();
    handler.lock().await.prune_destination_cache().await;
    let actual =
        restored.destination_identity(&learned.destination).await.expect("retained tunnel key");
    assert_eq!(actual.public_key_bytes(), expected.public_key_bytes());
    assert_eq!(actual.verifying_key_bytes(), expected.verifying_key_bytes());
    {
        let mut handler = handler.lock().await;
        handler.tunnel_table.tunnels.remove(&tunnel_ids[0]).expect("first tunnel");
        handler.prune_destination_cache().await;
        assert!(handler.single_out_destinations.contains_key(&learned.destination));
        handler.tunnel_table.remove_stale(now + Duration::from_secs(9 * 3600));
        handler.prune_destination_cache().await;
        assert!(!handler.single_out_destinations.contains_key(&learned.destination));
        assert_eq!(handler.single_out_destinations_removed_total, 1);
    }
}
