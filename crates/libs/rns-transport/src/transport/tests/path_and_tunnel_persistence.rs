#[tokio::test]
async fn announce_lookup_key_uses_destination_hash() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");

    let announced_destination = announce.destination;
    let announced_identity = *remote_destination.identity.address_hash();
    assert_ne!(
        announced_destination, announced_identity,
        "destination hash must differ from identity hash for named destinations"
    );

    let iface = AddressHash::new_from_rand(OsRng);
    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let guard = handler.lock().await;
    let keyed_by_destination = guard.announce_table.packet_for_destination(&announced_destination);
    assert!(keyed_by_destination.is_some(), "announce lookup should be keyed by destination hash");
    let keyed_by_identity = guard.announce_table.packet_for_destination(&announced_identity);
    assert!(keyed_by_identity.is_none(), "identity hash must not be used as announce lookup key");
}

#[tokio::test]
async fn known_remote_path_request_sends_path_response_context() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let (learned_iface, requesting_iface) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let learned_iface = *manager.new_channel(16).address();
        let requesting_iface = *manager.new_channel(16).address();
        (learned_iface, requesting_iface)
    };

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 2;
    let destination = announce.destination;
    let cached_data = announce.data.clone();

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x44; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, requesting_iface).await;
    }

    tokio::time::sleep(Duration::from_millis(450)).await;

    let messages = {
        let mut guard = handler.lock().await;
        let transport_id = *guard.config.identity.address_hash();
        guard.announce_table.drain_retransmissions(&transport_id)
    };

    assert_eq!(messages.len(), 1);
    let sent = &messages[0];
    assert!(
        matches!(sent.tx_type, TxMessageType::Direct(iface) if iface == requesting_iface),
        "known remote path responses should be direct to the requester"
    );
    assert_eq!(sent.packet.destination, destination);
    assert_eq!(sent.packet.context, PacketContext::PathResponse);
    assert_eq!(sent.packet.header.hops, 2);
    assert_eq!(sent.packet.data.as_slice(), cached_data.as_slice());
    assert!(
        !matches!(sent.tx_type, TxMessageType::Broadcast(Some(iface)) if iface == learned_iface),
        "path response must not rebroadcast on the learned-path ingress iface"
    );
}

#[tokio::test]
async fn path_response_holds_due_ordinary_announce_until_response_served() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let (learned_iface, requesting_iface) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let learned_iface = *manager.new_channel(16).address();
        let requesting_iface = *manager.new_channel(16).address();
        (learned_iface, requesting_iface)
    };

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 2;
    let destination = announce.destination;
    let cached_data = announce.data.clone();

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x45; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, requesting_iface).await;
    }

    tokio::time::sleep(Duration::from_millis(650)).await;

    let first = {
        let mut guard = handler.lock().await;
        let transport_id = *guard.config.identity.address_hash();
        guard.announce_table.drain_retransmissions(&transport_id)
    };

    assert_eq!(first.len(), 1);
    assert!(
        matches!(first[0].tx_type, TxMessageType::Direct(iface) if iface == requesting_iface),
        "known path responses should be served before a due ordinary announce"
    );
    assert_eq!(first[0].packet.destination, destination);
    assert_eq!(first[0].packet.context, PacketContext::PathResponse);
    assert_eq!(first[0].packet.data.as_slice(), cached_data.as_slice());

    let second = {
        let mut guard = handler.lock().await;
        let transport_id = *guard.config.identity.address_hash();
        guard.announce_table.drain_retransmissions(&transport_id)
    };

    assert_eq!(second.len(), 1);
    assert!(
        matches!(second[0].tx_type, TxMessageType::Broadcast(Some(iface)) if iface == learned_iface),
        "the held ordinary announce should rebroadcast after the path response"
    );
    assert_eq!(second[0].packet.destination, destination);
    assert_eq!(second[0].packet.context, PacketContext::None);
    assert_eq!(second[0].packet.data.as_slice(), cached_data.as_slice());
}

#[tokio::test]
async fn full_iface_answers_known_path_request_when_next_hop_is_same_iface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        *manager
            .new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            )
            .address()
    };

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 2;
    let destination = announce.destination;

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x55; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, iface).await;
    }

    let guard = handler.lock().await;
    let response = guard
        .announce_table
        .pending_response_for_destination(&destination)
        .expect("full-mode iface should schedule a known-path response");
    assert_eq!(response.response_to_iface, Some(iface));
    assert_eq!(response.hops, 2);
    assert_eq!(response.packet.context, PacketContext::PathResponse);
}

#[tokio::test]
async fn roaming_iface_suppresses_known_path_response_when_next_hop_is_same_iface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let iface = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        *manager
            .new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Roaming,
            )
            .address()
    };

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 2;
    let destination = announce.destination;

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x66; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, iface).await;
    }

    assert!(
        handler
            .lock()
            .await
            .announce_table
            .pending_response_for_destination(&destination)
            .is_none(),
        "roaming-mode iface should not answer when the known path was learned on the same iface"
    );
}

#[tokio::test]
async fn roaming_iface_delays_known_path_response_when_next_hop_differs() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let (learned_iface, requesting_iface) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let learned_iface = *manager
            .new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Full,
            )
            .address();
        let requesting_iface = *manager
            .new_channel_with_role_and_mode(
                16,
                crate::iface::IfaceRole::Unicast,
                crate::iface::InterfaceMode::Roaming,
            )
            .address();
        (learned_iface, requesting_iface)
    };

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 2;
    let destination = announce.destination;

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x77; crate::hash::ADDRESS_HASH_SIZE]))
    };

    let before_request = Instant::now();
    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, requesting_iface).await;
    }

    let guard = handler.lock().await;
    let response = guard
        .announce_table
        .pending_response_for_destination(&destination)
        .expect("roaming iface should schedule delayed known-path response");
    assert_eq!(response.response_to_iface, Some(requesting_iface));
    assert_eq!(response.hops, 2);
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    let response_delay =
        response.timeout.checked_duration_since(before_request).unwrap_or_default();
    assert!(
        response_delay >= super::announce_table::PATH_RESPONSE_ROAMING_GRACE,
        "roaming known-path responses should wait the extra Python roaming grace"
    );
}

#[tokio::test]
async fn reticulum_path_table_persistence_restores_route_and_identity_from_cached_announce() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    let expected_identity = *remote_destination.identity.as_identity();

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    assert!(temp.path().join("destination_table").exists());
    let destination_table = std::fs::read(temp.path().join("destination_table")).expect("read");
    let value: rmpv::Value =
        rmpv::decode::read_value(&mut std::io::Cursor::new(destination_table)).expect("msgpack");
    let rmpv::Value::Array(entries) = value else {
        panic!("destination_table must be an array");
    };
    let rmpv::Value::Array(fields) = &entries[0] else {
        panic!("destination_table entry must be an array");
    };
    let rmpv::Value::Binary(interface_hash) = &fields[6] else {
        panic!("interface hash must be binary");
    };
    assert_eq!(interface_hash.len(), crate::hash::HASH_SIZE);
    assert!(temp
        .path()
        .join("cache")
        .join("announces")
        .join(hex::encode(announce.hash().as_slice()))
        .exists());

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");
    let restore_report =
        restored.restore_reticulum_path_table_report(temp.path()).await.expect("restore");
    assert_eq!(restore_report.restored_active_paths, 1);
    assert_eq!(restore_report.restored_identities.len(), 1);
    assert_eq!(restore_report.restored_identities[0].destination, destination);
    assert_eq!(
        restore_report.restored_identities[0].public_key.as_slice(),
        expected_identity.public_key_bytes()
    );
    assert_eq!(
        restore_report.restored_identities[0].verifying_key.as_slice(),
        expected_identity.verifying_key_bytes()
    );
    let restored_identity = restored.destination_identity(&destination).await.expect("identity");
    assert_eq!(restored_identity.public_key_bytes(), expected_identity.public_key_bytes());
    assert_eq!(restored_identity.verifying_key_bytes(), expected_identity.verifying_key_bytes());
    assert!(restored.has_path(&destination).await, "path table entry should be restored");

    tokio::time::sleep(Duration::from_millis(550)).await;

    let restored_handler = restored.get_handler();
    let restored_messages = {
        let mut guard = restored_handler.lock().await;
        let transport_id = *guard.config.identity.address_hash();
        guard.announce_table.drain_retransmissions(&transport_id)
    };
    assert!(
        restored_messages.is_empty(),
        "restored cached announces must not be scheduled as fresh rebroadcasts"
    );

    let requesting_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let path_request = {
        let mut guard = restored_handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x77; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = restored_handler.lock().await;
        handle_path_request(&path_request, &mut guard, requesting_iface).await;
    }

    let guard = restored_handler.lock().await;
    let response = guard
        .announce_table
        .pending_response_for_destination(&destination)
        .expect("restored cached announce should answer known-path requests");
    assert_eq!(response.response_to_iface, Some(requesting_iface));
    assert_eq!(response.packet.context, PacketContext::PathResponse);
}

#[tokio::test]
async fn reticulum_path_table_save_filters_entries_without_cached_announces() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let stale_destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"stale-destination"));
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        assert!(handler.path_table.restore_tunnel_path(
            stale_destination,
            stale_destination,
            1,
            iface,
            Hash::new_from_slice(b"stale-packet"),
            std::time::Instant::now(),
        ));
    }

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);

    let payload = std::fs::read(temp.path().join("destination_table")).expect("read");
    let entries = super::path_table::PathTable::decode_python_entries(&payload).expect("decode");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].destination, destination);
    assert!(
        !entries.iter().any(|entry| entry.destination == stale_destination),
        "path entries without cached announce material must not be persisted"
    );
}

#[tokio::test]
async fn reticulum_path_table_restore_skips_when_connected_to_shared_instance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);

    let mut restored_config = TransportConfig::new("shared-instance-client", &local_identity, true);
    restored_config.set_retransmit(true);
    restored_config.set_connected_to_shared_instance(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");

    let restore_report =
        restored.restore_reticulum_path_table_report(temp.path()).await.expect("restore");
    assert_eq!(restore_report.restored_active_paths, 0);
    assert!(
        !restored.has_path(&destination).await,
        "shared-instance clients should not restore local path-table entries from storage"
    );
}

#[tokio::test]
async fn reticulum_path_table_restore_skips_expired_python_path_entries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    let table_path = temp.path().join("destination_table");
    let payload = std::fs::read(&table_path).expect("read destination table");
    let mut entries =
        super::path_table::PathTable::decode_python_entries(&payload).expect("decode entries");
    assert_eq!(entries.len(), 1);
    entries[0].expires_secs = 0.0;
    std::fs::write(
        &table_path,
        super::path_table::PathTable::encode_python_entries(&entries).expect("encode entries"),
    )
    .expect("write expired destination table");

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");

    let restore_report =
        restored.restore_reticulum_path_table_report(temp.path()).await.expect("restore");
    assert_eq!(restore_report.restored_active_paths, 0);
    assert_eq!(restore_report.skipped.active_expired, 1);
    assert!(
        !restored.has_path(&destination).await,
        "expired Python path-table entry must not restore stale route state"
    );
    assert!(
        restored.destination_identity(&destination).await.is_none(),
        "expired Python path-table entry must not restore cached identity material"
    );
}

#[tokio::test]
async fn reticulum_path_table_save_skips_when_connected_to_shared_instance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("shared-instance-client", &local_identity, true);
    config.set_retransmit(true);
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert!(transport.has_path(&destination).await, "test should have an in-memory path");
    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 0);
    assert!(
        !temp.path().join("destination_table").exists(),
        "shared-instance clients should not persist destination_table"
    );
    assert!(
        !temp.path().join("tunnels").exists(),
        "shared-instance clients should not persist tunnel table"
    );
}

#[tokio::test]
async fn transport_path_status_reports_known_route_metadata() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let destination = AddressHash::new_from_hash(&Hash::new_from_slice(b"destination"));
    let next_hop = AddressHash::new_from_hash(&Hash::new_from_slice(b"next-hop"));
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        assert!(handler.path_table.restore_tunnel_path(
            destination,
            next_hop,
            3,
            iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let status = transport.path_status(&destination).await;

    assert!(status.path_found);
    assert_eq!(status.destination, destination);
    assert_eq!(status.next_hop, Some(next_hop));
    assert_eq!(status.interface, Some(iface));
    assert_eq!(status.hops, Some(3));
}

#[tokio::test]
async fn reticulum_tunnel_table_persistence_restores_tunnel_paths_after_reappearance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;
    }

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    assert!(temp.path().join("tunnels").exists());
    let packet_hash = announce.hash();
    let packet_path =
        temp.path().join("cache").join("announces").join(hex::encode(packet_hash.as_slice()));
    assert!(packet_path.exists(), "active/tunnel snapshot packet is persisted");
    let tunnel_entries = super::tunnels::TunnelTable::decode_python_entries(
        &std::fs::read(temp.path().join("tunnels")).expect("read tunnel table"),
    )
    .expect("decode tunnel table");
    assert!(
        tunnel_entries
            .iter()
            .flat_map(|entry| &entry.paths)
            .any(|path| path.packet_hash == packet_hash),
        "the announce packet is referenced by the persisted tunnel snapshot"
    );

    let destination_table = temp.path().join("destination_table");
    let destination_table_before =
        std::fs::read(&destination_table).expect("read destination table");
    let destination_table_modified = std::fs::metadata(&destination_table)
        .expect("destination table metadata")
        .modified()
        .expect("destination table mtime");
    let announce_dir = temp.path().join("cache").join("announces");
    for index in 0..514u64 {
        let mut bytes = [0; crate::hash::HASH_SIZE];
        bytes[..8].copy_from_slice(&index.to_be_bytes());
        let orphan = announce_dir.join(hex::encode(bytes));
        std::fs::write(&orphan, b"obsolete cache entry").expect("write stale announce");
        std::fs::File::options()
            .write(true)
            .open(&orphan)
            .expect("open stale announce")
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1)),
            )
            .expect("age stale announce");
    }
    let mut examined = 0;
    let mut removed = 0;
    let mut bytes_removed = 0;
    let mut scan_complete = false;
    for _ in 0..3 {
        let cleanup = transport
            .clean_reticulum_announce_cache_batch(temp.path())
            .await
            .expect("cleanup batch")
            .expect("persisted references are available");
        assert!(cleanup.examined <= 512, "one pass is bounded");
        examined += cleanup.examined;
        removed += cleanup.removed;
        bytes_removed += cleanup.bytes_removed;
        scan_complete = cleanup.scan_complete;
        if scan_complete {
            break;
        }
    }
    assert!(scan_complete, "bounded batches eventually finish the directory pass");
    assert_eq!(examined, 515, "the orphan set and referenced packet are scanned");
    assert_eq!(removed, 514, "only unreferenced stale packets are deleted");
    assert_eq!(bytes_removed, 514 * b"obsolete cache entry".len() as u64);
    assert!(packet_path.exists(), "the tunnel-referenced packet survives cleanup");
    assert_eq!(
        std::fs::read(&destination_table).expect("destination table unchanged"),
        destination_table_before
    );
    assert_eq!(
        std::fs::metadata(&destination_table)
            .expect("destination table metadata")
            .modified()
            .expect("destination table mtime"),
        destination_table_modified,
        "maintenance-only cleanup must not rewrite the path-table snapshot"
    );
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 0);
    assert!(
        !restored.has_path(&destination).await,
        "tunnel table load should not restore active path before tunnel reappears"
    );

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        )
        .await;
    }

    assert!(
        restored.has_path(&destination).await,
        "tunnel reappearance should restore the persisted tunnel path"
    );
    assert!(restored.destination_identity(&destination).await.is_some());

    let requesting_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_handler = restored.get_handler();
    let path_request = {
        let mut guard = restored_handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x88; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = restored_handler.lock().await;
        handle_path_request(&path_request, &mut guard, requesting_iface).await;
    }

    let guard = restored_handler.lock().await;
    let response = guard
        .announce_table
        .pending_response_for_destination(&destination)
        .expect("restored tunnel cached announce should answer known-path requests");
    assert_eq!(response.response_to_iface, Some(requesting_iface));
    assert_eq!(response.packet.context, PacketContext::PathResponse);
}

#[tokio::test]
async fn reticulum_tunnel_table_restore_skips_expired_python_tunnel_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;
    }

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");
    let tunnel_path = temp.path().join("tunnels");
    let payload = std::fs::read(&tunnel_path).expect("read tunnels");
    let mut tunnels =
        super::tunnels::TunnelTable::decode_python_entries(&payload).expect("decode tunnels");
    assert_eq!(tunnels.len(), 1);
    assert_eq!(tunnels[0].paths.len(), 1);
    tunnels[0].paths[0].expires_secs = 0.0;
    std::fs::write(
        &tunnel_path,
        super::tunnels::TunnelTable::encode_python_entries(&tunnels).expect("encode tunnels"),
    )
    .expect("write expired tunnel path");

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    let restore_report =
        restored.restore_reticulum_path_table_report(temp.path()).await.expect("restore");
    assert_eq!(restore_report.restored_active_paths, 0);
    assert_eq!(restore_report.skipped.tunnel_expired, 1);

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        )
        .await;
    }

    assert!(
        !restored.has_path(&destination).await,
        "expired Python tunnel path must not restore stale route state"
    );
    assert!(
        restored.destination_identity(&destination).await.is_none(),
        "expired Python tunnel path must not restore cached identity material"
    );
}
