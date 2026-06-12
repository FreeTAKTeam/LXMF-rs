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

    let packet_hash = [0x55u8; HASH_SIZE];
    let signature = remote_destination.identity.sign(&packet_hash).to_bytes();
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&signature);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

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
    let config = TransportConfig::new("test", &local_identity, true);
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
