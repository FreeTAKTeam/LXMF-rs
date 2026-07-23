#[tokio::test]
async fn packet_signal_cache_returns_latest_value_and_evicts_like_python() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("signal-cache", &identity, true));
    let repeated = Hash::new_from_slice(b"repeated");

    transport
        .record_packet_signal(
            repeated,
            PacketSignal { rssi: Some(-70.0), snr: Some(3.0), q: Some(40.0) },
        )
        .await;
    transport
        .record_packet_signal(
            repeated,
            PacketSignal { rssi: Some(-65.0), snr: Some(5.0), q: Some(55.0) },
        )
        .await;
    assert_eq!(transport.packet_signal(&repeated).await.expect("signal").rssi, Some(-65.0));

    for index in 0..512_u64 {
        transport
            .record_packet_signal(
                Hash::new_from_slice(&index.to_be_bytes()),
                PacketSignal { q: Some(index as f64), ..Default::default() },
            )
            .await;
    }

    assert_eq!(transport.packet_signal(&repeated).await, None);
    let newest = Hash::new_from_slice(&511_u64.to_be_bytes());
    assert_eq!(transport.packet_signal(&newest).await.expect("newest").q, Some(511.0));
}

#[test]
fn next_hop_latency_metrics_match_python_formulas() {
    let metrics = NextHopMetrics {
        interface: AddressHash::new([1; crate::hash::ADDRESS_HASH_SIZE]),
        bitrate: 1_000,
        hardware_mtu: Some(500),
        per_bit_latency: Some(0.001),
    };
    assert_eq!(metrics.per_byte_latency(), Some(0.008));
    assert_eq!(metrics.extra_link_proof_timeout(500), Duration::from_secs(4));
    assert_eq!(
        metrics.first_hop_timeout(500, Duration::from_secs(6)),
        Duration::from_secs(10)
    );
}

#[tokio::test]
async fn await_path_returns_after_python_poll_interval_timeout() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("await-path", &identity, true));
    let unknown = AddressHash::new([0x44; crate::hash::ADDRESS_HASH_SIZE]);
    let started = tokio::time::Instant::now();
    assert!(!transport.await_path(&unknown, Duration::from_millis(55), None).await);
    assert!(started.elapsed() >= Duration::from_millis(50));
    assert_eq!(transport.hops_to(&unknown).await, PATHFINDER_M as u8);
}

#[tokio::test]
async fn destination_deregistration_and_interface_detach_release_runtime_state() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("deregister", &identity, true));
    let destination = transport
        .add_destination(
            PrivateIdentity::new_from_rand(OsRng),
            DestinationName::new("test", "destination"),
        )
        .await;
    let address = destination.lock().await.desc.address_hash;
    assert!(transport.has_destination(&address).await);
    assert!(transport.deregister_destination(&address).await);
    assert!(!transport.has_destination(&address).await);
    assert!(!transport.deregister_destination(&address).await);

    let full_hash = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let channel = manager.new_channel(4);
        manager.full_hash(channel.address()).expect("full hash")
    };
    assert!(transport.find_interface_from_hash(&full_hash).await.is_some());
    assert_eq!(transport.detach_interfaces().await, 1);
    assert!(transport.find_interface_from_hash(&full_hash).await.is_none());
}

#[tokio::test]
async fn network_identity_discovery_and_packet_cache_match_python_management_contracts() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("runtime-state", &identity, true));
    assert!(!transport.has_network_identity().await);
    assert!(transport.set_network_identity(PrivateIdentity::new_from_rand(OsRng)).await);
    assert!(transport.has_network_identity().await);
    assert!(!transport.set_network_identity(PrivateIdentity::new_from_rand(OsRng)).await);
    assert!(transport.enable_discovery().await);
    assert!(!transport.enable_discovery().await);
    assert!(transport.discovery_enabled().await);

    let temp = tempfile::tempdir().expect("tempdir");
    let packet = Packet::default();
    assert_eq!(
        transport
            .cache_packet(temp.path(), &packet, Some("iface"), false, false)
            .await
            .expect("skip cache"),
        None
    );
    let hash = transport
        .cache_packet(temp.path(), &packet, Some("iface"), true, false)
        .await
        .expect("cache")
        .expect("forced hash");
    let cached = transport
        .get_cached_packet(temp.path(), hash, false)
        .await
        .expect("read")
        .expect("cached packet");
    assert_eq!(cached.packet, packet);
    assert_eq!(cached.interface_reference.as_deref(), Some("iface"));
}

#[tokio::test]
async fn packet_hashlist_persistence_uses_python_messagepack_list() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("hashlist", &identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let packet = Packet::default();
    transport.get_handler().lock().await.packet_cache.lock().await.update(&packet);
    let temp = tempfile::tempdir().expect("tempdir");
    assert_eq!(transport.save_packet_hashlist(temp.path()).await.expect("save"), 1);
    let payload = std::fs::read(temp.path().join("packet_hashlist")).expect("hashlist");
    let value = rmpv::decode::read_value(&mut std::io::Cursor::new(payload)).expect("msgpack");
    assert_eq!(value.as_array().map(Vec::len), Some(1));
}
