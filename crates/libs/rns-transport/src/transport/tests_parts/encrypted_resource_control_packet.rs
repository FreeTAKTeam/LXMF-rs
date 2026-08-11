fn encrypted_resource_control_packet(
    link: &Link,
    context: PacketContext,
    payload: &[u8],
) -> Packet {
    let mut data = PacketDataBuffer::new();
    let cipher_len = {
        let cipher = link.encrypt(payload, data.accuire_buf_max()).expect("encrypt control packet");
        cipher.len()
    };
    data.resize(cipher_len);
    Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: *link.id(),
        context,
        data,
        ..Default::default()
    }
}

struct CountingReceiptHandler {
    count: Arc<AtomicUsize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestTypedMessage {
    value: Vec<u8>,
}

impl TypedMessage for TestTypedMessage {
    const MSG_TYPE: u16 = 0x7777;

    fn encode(&self) -> Vec<u8> {
        self.value.clone()
    }

    fn decode(payload: &[u8]) -> Result<Self, crate::channel::ChannelError> {
        Ok(Self { value: payload.to_vec() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReservedTypedMessage;

impl TypedMessage for ReservedTypedMessage {
    const MSG_TYPE: u16 = SystemMessageTypes::StreamData as u16;

    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    fn decode(_payload: &[u8]) -> Result<Self, crate::channel::ChannelError> {
        Ok(Self)
    }
}

impl ReceiptHandler for CountingReceiptHandler {
    fn on_receipt(&self, _receipt: &DeliveryReceipt) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn handle_inbound_for_test_rejects_forged_destination_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    let packet_hash = [0x44u8; HASH_SIZE];
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&[0xAA; ed25519_dalek::SIGNATURE_LENGTH]);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn handle_inbound_for_test_accepts_valid_destination_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    // Send a real packet to the remote destination first, so `PacketCache`
    // actually tracks this hash as one we sent (the fix for the
    // forged-receipt bug in `wire.rs::validated_receipt_hash`: a proof is
    // only trusted for a hash this instance tracked sending/relaying, never
    // just any hash paired with a signature from a known identity).
    let outbound = Packet {
        destination: announce.destination,
        data: PacketDataBuffer::new_from_slice(b"hello from the local sender"),
        ..Packet::default()
    };
    let trace = transport.send_packet_with_trace(outbound).await;
    let packet_hash = trace.packet_hash.expect("packet hash computed during send");

    let signature = remote_destination.identity.sign(packet_hash.as_slice()).to_bytes();
    let mut data = PacketDataBuffer::new();
    data.safe_write(packet_hash.as_slice());
    data.safe_write(&signature);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        // Real (and this crate's own) proof packets address to the hash of
        // the packet being proved, not the proving destination's real
        // address — see wire.rs's "meshage fork" comment.
        destination: AddressHash::new_from_hash(&packet_hash),
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handle_inbound_for_test_rejects_untracked_hash_even_with_a_known_identitys_signature() {
    // Regression test for a forged-receipt bug: a known peer could sign any
    // hash it had merely observed (e.g. one addressed to a different
    // destination) and have it accepted as a delivery receipt, because the
    // old fallback in `validated_receipt_hash` tried every known
    // destination's identity against an explicit proof whenever there was
    // no cached record for it. It must now be rejected outright — the
    // signature is valid, but this instance never sent or relayed anything
    // with this hash, so there's nothing to check the proof against.
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    // Never sent or relayed — a hash the remote destination is happy to
    // sign (it's genuinely valid under its own identity) but that this
    // instance has no record of ever transmitting.
    let untracked_hash = [0x55u8; HASH_SIZE];
    let signature = remote_destination.identity.sign(&untracked_hash).to_bytes();
    let mut data = PacketDataBuffer::new();
    data.safe_write(&untracked_hash);
    data.safe_write(&signature);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: AddressHash::new_from_hash(&Hash::new(untracked_hash)),
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn routed_destination_proof_forwards_back_to_packet_source() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut source_iface = transport.iface_manager.lock().await.new_channel(8);
    let mut recipient_iface = transport.iface_manager.lock().await.new_channel(8);

    let recipient_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut recipient_destination =
        SingleInputDestination::new(recipient_identity, DestinationName::new("lxmf", "delivery"));
    let announce = recipient_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        recipient_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let original_packet = Packet {
        header: Header { packet_type: PacketType::Data, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(b"opportunistic lxmf body"),
        ..Default::default()
    };

    assert!(handler.lock().await.filter_duplicate_packets(&original_packet).await);
    handle_data(&original_packet, source_iface.address, handler.lock().await).await;
    let forwarded = timeout(Duration::from_millis(200), recipient_iface.tx_channel.recv())
        .await
        .expect("data should be forwarded to recipient iface")
        .expect("tx channel open");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(recipient_iface.address));

    let packet_hash = original_packet.hash().to_bytes();
    let signature = recipient_destination.identity.sign(&packet_hash).to_bytes();
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&signature);
    let proof = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    handle_proof(proof, handler, recipient_iface.address).await;

    let sent = timeout(Duration::from_millis(200), source_iface.tx_channel.recv())
        .await
        .expect("destination proof should be forwarded back to packet source")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, TxMessageType::Direct(source_iface.address));
    assert_eq!(sent.packet.header.packet_type, PacketType::Proof);
    assert_eq!(sent.packet.destination, announce.destination);
}

#[tokio::test]
async fn routed_implicit_destination_proof_forwards_back_to_packet_source() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut source_iface = transport.iface_manager.lock().await.new_channel(8);
    let mut recipient_iface = transport.iface_manager.lock().await.new_channel(8);

    let recipient_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut recipient_destination =
        SingleInputDestination::new(recipient_identity, DestinationName::new("lxmf", "delivery"));
    let announce = recipient_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        recipient_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let original_packet = Packet {
        header: Header { packet_type: PacketType::Data, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(b"sideband implicit lxmf body"),
        ..Default::default()
    };

    assert!(handler.lock().await.filter_duplicate_packets(&original_packet).await);
    handle_data(&original_packet, source_iface.address, handler.lock().await).await;
    let forwarded = timeout(Duration::from_millis(200), recipient_iface.tx_channel.recv())
        .await
        .expect("data should be forwarded to recipient iface")
        .expect("tx channel open");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(recipient_iface.address));

    let packet_hash = original_packet.hash();
    let signature = recipient_destination.identity.sign(packet_hash.as_slice()).to_bytes();
    let proof = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: AddressHash::new_from_hash(&packet_hash),
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(&signature),
        ..Default::default()
    };

    handle_proof(proof, handler, recipient_iface.address).await;

    let sent = timeout(Duration::from_millis(200), source_iface.tx_channel.recv())
        .await
        .expect("implicit destination proof should be forwarded back to packet source")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, TxMessageType::Direct(source_iface.address));
    assert_eq!(sent.packet.header.packet_type, PacketType::Proof);
    assert_eq!(sent.packet.destination, AddressHash::new_from_hash(&packet_hash));
    assert_eq!(sent.packet.data.as_slice(), signature.as_slice());
}

#[tokio::test]
async fn handle_inbound_for_test_accepts_python_style_link_proof_with_none_context() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound =
        Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        LinkHandleResult::Activated
    ));

    let packet = outbound.data_packet(b"python proof context").expect("link packet");
    let mut proof = match inbound.handle_packet(&packet, iface) {
        LinkHandleResult::Proof(proof) => proof,
        _ => panic!("link packet should generate proof"),
    };
    proof.context = PacketContext::None;
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    transport.handle_inbound_for_test(proof).await;

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn routed_link_request_proof_requires_matching_iface_and_signature() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link =
        crate::destination::link::Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    handle_link_request_as_intermediate(
        received_from,
        next_hop,
        next_hop_iface,
        &request,
        handler.lock().await,
    )
    .await;

    let mut inbound_link = crate::destination::link::Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");

    let valid_proof = inbound_link.prove();
    handle_proof(valid_proof, handler.clone(), AddressHash::new_from_slice(&[9u8; 16])).await;
    {
        let guard = handler.lock().await;
        assert!(
            guard.link_table.original_destination(outbound_link.id()).is_none(),
            "proof from wrong interface must not validate"
        );
    }

    let mut bad_signature_proof = inbound_link.prove();
    bad_signature_proof.data.as_mut_slice()[0] ^= 0x01;
    handle_proof(bad_signature_proof, handler.clone(), next_hop_iface).await;
    {
        let guard = handler.lock().await;
        assert!(
            guard.link_table.original_destination(outbound_link.id()).is_none(),
            "invalid proof signature must not validate"
        );
    }

    let valid_proof = inbound_link.prove();
    handle_proof(valid_proof, handler.clone(), next_hop_iface).await;
    {
        let guard = handler.lock().await;
        assert_eq!(
            guard.link_table.original_destination(outbound_link.id()),
            Some(request.destination)
        );
    }
}

#[tokio::test]
async fn routed_link_request_clamps_forwarded_mtu_signalling_to_next_hop_iface() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (ingress_iface, mut next_hop_channel) = {
        let iface_manager = transport.iface_manager();
        let mut manager = iface_manager.lock().await;
        let ingress = manager.new_channel_with_role_mode_mtu(
            16,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            448,
        );
        let next_hop = manager.new_channel_with_role_mode_mtu(
            16,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            384,
        );
        (*ingress.address(), next_hop)
    };
    let next_hop_iface = *next_hop_channel.address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = remote_destination.desc.address_hash;
    let next_hop = AddressHash::new_from_rand(OsRng);
    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            next_hop,
            2,
            next_hop_iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let requester = PrivateIdentity::new_from_rand(OsRng);
    let mut data = PacketDataBuffer::new();
    data.safe_write(requester.as_identity().public_key.as_bytes());
    data.safe_write(requester.as_identity().verifying_key.as_bytes());
    data.safe_write(&[0x20, 0x20, 0x00]);
    let request = Packet {
        header: Header { packet_type: PacketType::LinkRequest, ..Default::default() },
        destination: destination_hash,
        context: PacketContext::None,
        data,
        ..Default::default()
    };
    let key_material_len = crate::identity::PUBLIC_KEY_LENGTH * 2;
    let original_key_material = request.data.as_slice()[..key_material_len].to_vec();

    handle_link_request_as_intermediate(
        ingress_iface,
        next_hop,
        next_hop_iface,
        &request,
        handler.lock().await,
    )
    .await;

    let forwarded = timeout(Duration::from_millis(200), next_hop_channel.tx_channel.recv())
        .await
        .expect("link request should forward to next-hop iface")
        .expect("tx channel open");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(next_hop_iface));
    assert_eq!(forwarded.packet.header.packet_type, PacketType::LinkRequest);
    assert_eq!(&forwarded.packet.data.as_slice()[..key_material_len], original_key_material);
    assert_eq!(
        &forwarded.packet.data.as_slice()[key_material_len..key_material_len + 3],
        &[0x20, 0x01, 0x80],
        "forwarded MTU signalling should preserve mode bits and clamp to the next-hop iface MTU"
    );
    assert_eq!(
        &request.data.as_slice()[key_material_len..key_material_len + 3],
        &[0x20, 0x20, 0x00],
        "intermediate forwarding must not mutate the caller-owned packet"
    );
}

#[tokio::test]
async fn routed_link_request_preserves_python_default_mtu_signalling_when_ifaces_allow() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (ingress_iface, mut next_hop_channel) = {
        let iface_manager = transport.iface_manager();
        let mut manager = iface_manager.lock().await;
        let ingress = manager.new_channel_with_role_mode_mtu(
            16,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            500,
        );
        let next_hop = manager.new_channel_with_role_mode_mtu(
            16,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            500,
        );
        (*ingress.address(), next_hop)
    };
    let next_hop_iface = *next_hop_channel.address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = remote_destination.desc.address_hash;
    let next_hop = AddressHash::new_from_rand(OsRng);
    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            next_hop,
            2,
            next_hop_iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let requester = PrivateIdentity::new_from_rand(OsRng);
    let mut data = PacketDataBuffer::new();
    data.safe_write(requester.as_identity().public_key.as_bytes());
    data.safe_write(requester.as_identity().verifying_key.as_bytes());
    data.safe_write(&[0x20, 0x01, 0xF4]);
    let request = Packet {
        header: Header {
            packet_type: PacketType::LinkRequest,
            ..Default::default()
        },
        destination: destination_hash,
        context: PacketContext::None,
        data,
        ..Default::default()
    };
    let key_material_len = crate::identity::PUBLIC_KEY_LENGTH * 2;

    handle_link_request_as_intermediate(
        ingress_iface,
        next_hop,
        next_hop_iface,
        &request,
        handler.lock().await,
    )
    .await;

    let forwarded = timeout(Duration::from_millis(200), next_hop_channel.tx_channel.recv())
        .await
        .expect("link request should forward to next-hop iface")
        .expect("tx channel open");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(next_hop_iface));
    assert_eq!(
        &forwarded.packet.data.as_slice()[key_material_len..key_material_len + 3],
        &[0x20, 0x01, 0xF4],
        "Python default 500-byte MTU signalling must not be rewritten to 499"
    );
}

#[tokio::test]
async fn routed_link_request_without_mtu_signalling_forwards_without_appending_bytes() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (ingress_iface, mut next_hop_channel) = {
        let iface_manager = transport.iface_manager();
        let mut manager = iface_manager.lock().await;
        let ingress = manager.new_channel_with_role_mode_mtu(
            16,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            448,
        );
        let next_hop = manager.new_channel_with_role_mode_mtu(
            16,
            crate::iface::IfaceRole::Unicast,
            crate::iface::InterfaceMode::Full,
            384,
        );
        (*ingress.address(), next_hop)
    };
    let next_hop_iface = *next_hop_channel.address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let destination_hash = remote_destination.desc.address_hash;
    let next_hop = AddressHash::new_from_rand(OsRng);
    {
        let mut guard = handler.lock().await;
        assert!(guard.path_table.restore_tunnel_path(
            destination_hash,
            next_hop,
            2,
            next_hop_iface,
            Hash::new_from_slice(b"packet"),
            std::time::Instant::now(),
        ));
    }

    let requester = PrivateIdentity::new_from_rand(OsRng);
    let mut data = PacketDataBuffer::new();
    data.safe_write(requester.as_identity().public_key.as_bytes());
    data.safe_write(requester.as_identity().verifying_key.as_bytes());
    let request = Packet {
        header: Header { packet_type: PacketType::LinkRequest, ..Default::default() },
        destination: destination_hash,
        context: PacketContext::None,
        data,
        ..Default::default()
    };
    let original_data = request.data.as_slice().to_vec();

    handle_link_request_as_intermediate(
        ingress_iface,
        next_hop,
        next_hop_iface,
        &request,
        handler.lock().await,
    )
    .await;

    let forwarded = timeout(Duration::from_millis(200), next_hop_channel.tx_channel.recv())
        .await
        .expect("link request should forward to next-hop iface")
        .expect("tx channel open");
    assert_eq!(forwarded.tx_type, TxMessageType::Direct(next_hop_iface));
    assert_eq!(forwarded.packet.header.packet_type, PacketType::LinkRequest);
    assert_eq!(forwarded.packet.data.as_slice(), original_data);
    assert_eq!(request.data.as_slice(), original_data);
}

#[test]
fn link_request_proof_starts_with_zero_hops() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination, tx.clone());
    let mut request = outbound.request();
    request.header.hops = 2;

    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let proof = inbound.prove();

    assert_eq!(proof.context, PacketContext::LinkRequestProof);
    assert_eq!(proof.header.hops, 0);
}

#[tokio::test]
async fn routed_link_request_proof_preserves_wire_shape_when_forwarded_backwards() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let mut link_table = LinkTable::new(Duration::from_secs(5), Duration::from_secs(30));
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let mut request = outbound_link.request();
    request.header.hops = 1;
    link_table.add(&request, request.destination, received_from, next_hop, next_hop_iface);

    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let proof = inbound.prove();
    let (forwarded, target) = link_table.handle_proof(&proof).expect("forwarded proof");

    assert_eq!(target, received_from);
    assert_eq!(forwarded.context, PacketContext::LinkRequestProof);
    assert_eq!(forwarded.header.header_type, HeaderType::Type1);
    assert_eq!(forwarded.transport, None);
    assert_eq!(forwarded.destination, proof.destination);
    assert_eq!(forwarded.header.hops, proof.header.hops);
}

#[tokio::test]
async fn routed_link_resource_request_forwards_back_to_link_requester() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let mut link_table = LinkTable::new(Duration::from_secs(5), Duration::from_secs(30));
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    link_table.add(&request, request.destination, received_from, next_hop, next_hop_iface);

    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let proof = inbound.prove();
    assert!(link_table.handle_proof(&proof).is_some());

    let resource_request = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: *outbound_link.id(),
        context: PacketContext::ResourceRequest,
        data: PacketDataBuffer::new_from_slice(b"resource request"),
        ..Default::default()
    };

    let (forwarded, target) = link_table
        .handle_reverse_link_packet(&resource_request, next_hop_iface)
        .expect("reverse link packet should forward");
    assert_eq!(target, received_from);
    assert_eq!(forwarded.destination, resource_request.destination);
    assert_eq!(forwarded.context, PacketContext::ResourceRequest);

    assert!(
        link_table.handle_reverse_link_packet(&resource_request, received_from).is_none(),
        "requester-side packets should keep using the normal forward path"
    );
}

#[tokio::test]
async fn routed_link_resource_proof_forwards_back_to_link_requester() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut requester_iface = transport.iface_manager.lock().await.new_channel(8);
    let received_from = requester_iface.address;

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));

    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let link_request_proof = inbound.prove();
    assert!(matches!(
        outbound_link.handle_packet(&link_request_proof, next_hop_iface),
        LinkHandleResult::Activated
    ));

    {
        let mut guard = handler.lock().await;
        guard.link_table.add(
            &request,
            request.destination,
            received_from,
            next_hop,
            next_hop_iface,
        );
        assert!(guard.link_table.handle_proof(&link_request_proof).is_some());
    }

    let proof_payload = ResourceProof {
        resource_hash: crate::hash::Hash::new_from_slice(&[0x44; 32]),
        proof: crate::hash::Hash::new_from_slice(&[0x55; 32]),
    };
    let resource_proof = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        destination: *outbound_link.id(),
        context: PacketContext::ResourceProof,
        data: PacketDataBuffer::new_from_slice(&proof_payload.encode()),
        ..Default::default()
    };

    handle_proof(resource_proof, handler, next_hop_iface).await;

    let sent = timeout(Duration::from_millis(200), requester_iface.tx_channel.recv())
        .await
        .expect("resource proof should be forwarded back to requester iface")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, TxMessageType::Direct(received_from));
    assert_eq!(sent.packet.destination, *outbound_link.id());
    assert_eq!(sent.packet.header.packet_type, PacketType::Proof);
    assert_eq!(sent.packet.context, PacketContext::ResourceProof);
}

#[tokio::test]
async fn routed_reverse_transfer_resource_proof_forwards_to_link_responder() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let requester_iface = transport.iface_manager.lock().await.new_channel(8);
    let mut responder_iface = transport.iface_manager.lock().await.new_channel(8);

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        responder_iface.address,
        crate::iface::IfaceSource::None,
    )
    .await;

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let link_request_proof = inbound.prove();

    {
        let mut guard = handler.lock().await;
        guard.link_table.add(
            &request,
            request.destination,
            requester_iface.address,
            request.destination,
            responder_iface.address,
        );
        assert!(guard.link_table.handle_proof(&link_request_proof).is_some());
    }

    let proof_payload = ResourceProof {
        resource_hash: crate::hash::Hash::new_from_slice(&[0x66; 32]),
        proof: crate::hash::Hash::new_from_slice(&[0x77; 32]),
    };
    let resource_proof = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        destination: *outbound_link.id(),
        context: PacketContext::ResourceProof,
        data: PacketDataBuffer::new_from_slice(&proof_payload.encode()),
        ..Default::default()
    };

    handle_proof(resource_proof, handler, requester_iface.address).await;

    let sent = timeout(Duration::from_millis(200), responder_iface.tx_channel.recv())
        .await
        .expect("resource proof should be forwarded to responder iface")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, TxMessageType::Direct(responder_iface.address));
    assert_eq!(sent.packet.destination, *outbound_link.id());
    assert_eq!(sent.packet.header.packet_type, PacketType::Proof);
    assert_eq!(sent.packet.context, PacketContext::ResourceProof);
}

#[tokio::test]
async fn routed_link_packet_proof_forwards_back_to_link_requester() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut requester_iface = transport.iface_manager.lock().await.new_channel(8);
    let received_from = requester_iface.address;

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));

    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let link_request_proof = inbound.prove();

    {
        let mut guard = handler.lock().await;
        guard.link_table.add(
            &request,
            request.destination,
            received_from,
            next_hop,
            next_hop_iface,
        );
        assert!(guard.link_table.handle_proof(&link_request_proof).is_some());
    }

    let data_packet = outbound_link.data_packet(b"needs receipt proof").expect("data packet");
    let packet_proof = inbound.prove_packet(&data_packet);

    handle_proof(packet_proof, handler, next_hop_iface).await;

    let sent = timeout(Duration::from_millis(200), requester_iface.tx_channel.recv())
        .await
        .expect("packet proof should be forwarded back to requester iface")
        .expect("tx channel open");
    assert_eq!(sent.tx_type, TxMessageType::Direct(received_from));
    assert_eq!(sent.packet.destination, *outbound_link.id());
    assert_eq!(sent.packet.header.packet_type, PacketType::Proof);
    assert!(matches!(sent.packet.context, PacketContext::None | PacketContext::LinkProof));
}

#[tokio::test]
async fn disabled_transport_does_not_forward_established_link_traffic() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("disabled-transport", &local_identity, true);
    config.set_transport_enabled(false);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (mut requester_iface, next_hop_iface) = {
        let iface_manager = transport.iface_manager();
        let mut manager = iface_manager.lock().await;
        let requester = manager.new_channel(8);
        let next_hop = manager.new_channel(8);
        (requester, *next_hop.address())
    };
    let received_from = *requester_iface.address();

    let remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "disabled-transit"),
    );
    let next_hop = AddressHash::new_from_rand(OsRng);
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    let mut inbound_link = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let link_request_proof = inbound_link.prove();
    assert!(matches!(
        outbound_link.handle_packet(&link_request_proof, next_hop_iface),
        LinkHandleResult::Activated
    ));

    {
        let mut guard = handler.lock().await;
        guard.link_table.add(
            &request,
            request.destination,
            received_from,
            next_hop,
            next_hop_iface,
        );
        assert!(guard.link_table.handle_proof(&link_request_proof).is_some());
    }

    let data_packet = outbound_link.data_packet(b"must not transit").expect("link data packet");
    handle_data(&data_packet, next_hop_iface, handler.lock().await).await;

    assert!(
        timeout(Duration::from_millis(200), requester_iface.tx_channel.recv()).await.is_err(),
        "disabled transport must not forward established-link data"
    );

    let packet_proof = inbound_link.prove_packet(&data_packet);
    handle_proof(packet_proof, handler, next_hop_iface).await;

    assert!(
        timeout(Duration::from_millis(200), requester_iface.tx_channel.recv()).await.is_err(),
        "disabled transport must not forward established-link proofs"
    );
}
