#[tokio::test]
async fn routed_link_packets_follow_recorded_interfaces_without_a_destination_path() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("packet-proof-routing", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let (mut requester, mut responder) = {
        let mut manager = transport.iface_manager.lock().await;
        (manager.new_channel(8), manager.new_channel(8))
    };
    let destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination.desc, tx.clone());
    let request = outbound.request();
    let mut inbound =
        Link::new_from_request(&request, destination.sign_key().clone(), destination.desc, tx)
            .expect("link from request");
    let establishment_proof = inbound.prove();
    {
        let mut guard = handler.lock().await;
        guard.link_table.add(
            &request,
            request.destination,
            requester.address,
            destination.desc.address_hash,
            responder.address,
        );
        assert!(guard.link_table.handle_proof(&establishment_proof).is_some());
    }
    let data = outbound.data_packet(b"link delivery proof").expect("link packet");
    let proof = inbound.prove_packet(&data);

    for (packet_type, context) in [
        (PacketType::Proof, PacketContext::None),
        (PacketType::Proof, PacketContext::LinkProof),
        (PacketType::Data, PacketContext::None),
        (PacketType::Data, PacketContext::LinkRTT),
        (PacketType::Data, PacketContext::ResourceRequest),
        (PacketType::Proof, PacketContext::ResourceProof),
    ] {
        let mut proof = proof.clone();
        proof.header.packet_type = packet_type;
        proof.context = context;
        for from_requester in [true, false] {
            let (source, target) = if from_requester {
                (requester.address, &mut responder)
            } else {
                (responder.address, &mut requester)
            };
            if packet_type == PacketType::Proof {
                handle_proof(proof.clone(), handler.clone(), source).await;
            } else {
                handle_data(&proof, source, handler.lock().await).await;
            }
            let sent = timeout(Duration::from_millis(200), target.tx_channel.recv())
                .await
                .expect("Link packet must reach the opposite recorded interface without a path")
                .expect("interface channel open");
            assert_eq!(sent.tx_type, TxMessageType::Direct(target.address));
            assert_eq!(sent.packet.header, proof.header);
            assert_eq!(sent.packet.destination, proof.destination);
            assert_eq!(sent.packet.context, proof.context);
            assert_eq!(sent.packet.data.as_slice(), proof.data.as_slice());
            assert!(requester.tx_channel.try_recv().is_err());
            assert!(responder.tx_channel.try_recv().is_err());
        }

        let unrelated = AddressHash::new_from_rand(OsRng);
        if packet_type == PacketType::Proof {
            handle_proof(proof, handler.clone(), unrelated).await;
        } else {
            handle_data(&proof, unrelated, handler.lock().await).await;
        }
        assert!(requester.tx_channel.try_recv().is_err(), "unrelated ingress must not transit");
        assert!(responder.tx_channel.try_recv().is_err(), "unrelated ingress must not transit");
    }

    let mut pending = Link::new(destination.desc, tokio::sync::broadcast::channel(4).0);
    let pending_request = pending.request();
    handler.lock().await.link_table.add(
        &pending_request,
        pending_request.destination,
        requester.address,
        destination.desc.address_hash,
        responder.address,
    );
    let mut unvalidated_proof = proof;
    unvalidated_proof.destination = *pending.id();
    handle_proof(unvalidated_proof, handler.clone(), responder.address).await;
    assert!(requester.tx_channel.try_recv().is_err(), "pending Link must not transit");
    assert!(responder.tx_channel.try_recv().is_err(), "pending Link must not transit");
    assert!(handler.lock().await.link_table.original_destination(pending.id()).is_none());
}
