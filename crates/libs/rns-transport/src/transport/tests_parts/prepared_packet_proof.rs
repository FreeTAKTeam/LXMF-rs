#[tokio::test]
async fn prepared_single_rejects_invalid_binding_before_observation_or_handoff() {
    let local = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("prepared-binding", &local, true));
    let mut channel = transport.iface_manager().lock().await.new_channel(16);
    let remote = PrivateIdentity::new_from_rand(OsRng);
    let name = DestinationName::new("lxmf", "delivery");
    let identity = *remote.as_identity();
    let destination = SingleOutputDestination::new(identity, name).desc.address_hash;
    let packet = Packet {
        destination,
        data: PacketDataBuffer::new_from_slice(b"prepared ciphertext"),
        ..Packet::default()
    };
    let mut forged = identity;
    forged.address_hash = AddressHash::new_from_rand(OsRng);
    let mut forged_packet = packet.clone();
    forged_packet.destination = SingleOutputDestination::new(forged, name).desc.address_hash;
    let observed = AtomicUsize::new(0);
    for (candidate, recipient) in
        [(packet, forged), (forged_packet.clone(), forged), (forged_packet, identity)]
    {
        assert!(transport
            .send_prepared_single_packet_observed_with_trace(candidate, recipient, name, |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .is_err());
    }
    assert_eq!(observed.load(Ordering::SeqCst), 0);
    assert!(channel.tx_channel.try_recv().is_err());
    assert!(transport.get_handler().lock().await.single_out_destinations.is_empty());
}

#[tokio::test]
async fn prepared_single_preserves_conflicting_known_identity() {
    let local = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("prepared-conflict", &local, true));
    let mut channel = transport.iface_manager().lock().await.new_channel(16);
    let remote = PrivateIdentity::new_from_rand(OsRng);
    let name = DestinationName::new("lxmf", "delivery");
    let destination = SingleOutputDestination::new(*remote.as_identity(), name).desc.address_hash;
    let other = PrivateIdentity::new_from_rand(OsRng);
    let existing = Arc::new(Mutex::new(SingleOutputDestination::new(*other.as_identity(), name)));
    transport
        .get_handler()
        .lock()
        .await
        .single_out_destinations
        .insert(destination, existing.clone());
    let observed = AtomicUsize::new(0);
    let result = transport
        .send_prepared_single_packet_observed_with_trace(
            Packet { destination, ..Packet::default() },
            *remote.as_identity(),
            name,
            |_| {
                observed.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
    assert!(result.is_err());
    assert_eq!(observed.load(Ordering::SeqCst), 0);
    assert!(channel.tx_channel.try_recv().is_err());
    let handler = transport.get_handler();
    let guard = handler.lock().await;
    assert!(Arc::ptr_eq(
        guard.single_out_destinations.get(&destination).expect("entry retained"),
        &existing
    ));
}

#[tokio::test]
async fn prepared_single_proofs_remain_bound_to_tracked_packet_and_recipient() {
    for explicit in [false, true] {
        let local = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new("prepared-proof", &local, true));
        let mut channel = transport.iface_manager().lock().await.new_channel(16);
        let recipient = PrivateIdentity::new_from_rand(OsRng);
        let wrong_signer = PrivateIdentity::new_from_rand(OsRng);
        let name = DestinationName::new("lxmf", "delivery");
        let destination =
            SingleOutputDestination::new(*recipient.as_identity(), name).desc.address_hash;
        let packet = Packet {
            destination,
            data: PacketDataBuffer::new_from_slice(b"exact prepared ciphertext"),
            ..Packet::default()
        };
        let hash = packet.hash();
        let trace = transport
            .send_prepared_single_packet_observed_with_trace(
                packet.clone(),
                *recipient.as_identity(),
                name,
                |observed| {
                    assert_eq!(observed, hash);
                },
            )
            .await
            .expect("bound dispatch");
        assert_eq!(trace.outcome, SendPacketOutcome::SentBroadcast);
        let sent = channel.tx_channel.recv().await.expect("broadcast");
        assert_eq!(sent.packet.hash(), hash);
        assert_eq!(sent.packet.data.as_slice(), packet.data.as_slice());
        assert!(!transport.has_path(&destination).await);

        // Register another known signer, so rejection cannot rely on unknown identity.
        let other = SingleOutputDestination::new(*wrong_signer.as_identity(), name);
        transport
            .get_handler()
            .lock()
            .await
            .single_out_destinations
            .insert(other.desc.address_hash, Arc::new(Mutex::new(other)));
        let count = Arc::new(AtomicUsize::new(0));
        transport
            .set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() }))
            .await;
        let untracked = Hash::new([0x55; HASH_SIZE]);
        for (proved_hash, signer, accepted) in
            [(hash, &wrong_signer, false), (untracked, &recipient, false), (hash, &recipient, true)]
        {
            let mut data = PacketDataBuffer::new();
            if explicit {
                data.safe_write(proved_hash.as_slice());
            }
            data.safe_write(&signer.sign(proved_hash.as_slice()).to_bytes());
            transport
                .handle_inbound_for_test(Packet {
                    header: Header { packet_type: PacketType::Proof, ..Default::default() },
                    destination: AddressHash::new_from_hash(&proved_hash),
                    data,
                    ..Packet::default()
                })
                .await;
            assert_eq!(count.load(Ordering::SeqCst), usize::from(accepted));
        }
    }
}
#[tokio::test]
async fn prepared_single_guard_rechecks_after_cache_and_interface_locks() {
    for block_cache in [true, false] {
        let local = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new("guarded-prepared", &local, true));
        let interfaces = transport.iface_manager();
        let mut channel = interfaces.lock().await.new_channel(8);
        let cache = transport.get_handler().lock().await.packet_cache.clone();
        let remote = PrivateIdentity::new_from_rand(OsRng);
        let name = DestinationName::new("lxmf", "delivery");
        let destination =
            SingleOutputDestination::new(*remote.as_identity(), name).desc.address_hash;
        let packet = Packet {
            destination,
            data: PacketDataBuffer::new_from_slice(b"prepared"),
            ..Packet::default()
        };
        let hash = packet.hash();
        let allowed = AtomicUsize::new(1);
        let correlation_count = AtomicUsize::new(0);
        let cache_guard = if block_cache { Some(cache.lock().await) } else { None };
        let interface_guard = if block_cache { None } else { Some(interfaces.lock().await) };
        let send = transport.send_prepared_single_packet_guarded_with_trace(
            packet,
            *remote.as_identity(),
            name,
            |_| {
                if allowed.load(Ordering::SeqCst) == 0 {
                    return false;
                }
                correlation_count.fetch_add(1, Ordering::SeqCst);
                true
            },
        );
        tokio::pin!(send);
        tokio::select! {
            biased;
            _ = &mut send => panic!("handoff cannot finish while its lock is held"),
            _ = tokio::task::yield_now() => {},
        }
        assert_eq!(correlation_count.load(Ordering::SeqCst), 0);
        allowed.store(0, Ordering::SeqCst);
        drop(cache_guard);
        drop(interface_guard);
        assert!(send.await.expect("valid binding").is_none());
        assert_eq!(correlation_count.load(Ordering::SeqCst), 0);
        assert!(cache
            .lock()
            .await
            .proof_context_for_destination(&AddressHash::new_from_hash(&hash))
            .is_none());
        assert!(channel.tx_channel.try_recv().is_err());
    }
}
