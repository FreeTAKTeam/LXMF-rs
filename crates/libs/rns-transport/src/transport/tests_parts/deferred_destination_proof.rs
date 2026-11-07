use super::*;

#[tokio::test]
async fn deferred_destination_proof_retry_scope_preserves_other_routes_and_contexts() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("deferred-scope", &identity, true));
    let deferred = transport
        .add_destination_with_deferred_proofs(
            identity.clone(),
            DestinationName::new("lxmf", "delivery"),
        )
        .await;
    let ordinary = transport
        .add_destination(identity.clone(), DestinationName::new("other", "delivery"))
        .await;
    assert!(!ordinary.lock().await.deferred_proofs());
    let packet =
        Packet { destination: deferred.lock().await.desc.address_hash, ..Packet::default() };
    let handler = transport.handler.lock().await;
    assert!(handler.allows_deferred_destination_retry(&packet).await);
    let mut wrong_route = packet.clone();
    wrong_route.transport = Some(*PrivateIdentity::new_from_rand(OsRng).address_hash());
    assert!(!handler.allows_deferred_destination_retry(&wrong_route).await);
    let mut nonlocal = packet.clone();
    nonlocal.destination = *PrivateIdentity::new_from_rand(OsRng).address_hash();
    assert!(!handler.allows_deferred_destination_retry(&nonlocal).await);
    let mut ordinary_packet = packet.clone();
    ordinary_packet.destination = ordinary.lock().await.desc.address_hash;
    assert!(!handler.allows_deferred_destination_retry(&ordinary_packet).await);
    for context in [
        PacketContext::Request,
        PacketContext::Response,
        PacketContext::Resource,
        PacketContext::KeepAlive,
    ] {
        let mut control = packet.clone();
        control.context = context;
        assert!(!handler.allows_deferred_destination_retry(&control).await);
    }
    for packet_type in [PacketType::Announce, PacketType::Proof, PacketType::LinkRequest] {
        let mut control = packet.clone();
        control.header.packet_type = packet_type;
        assert!(!handler.allows_deferred_destination_retry(&control).await);
    }
}

#[tokio::test]
async fn deferred_destination_proof_is_explicit_bounded_and_preserves_legacy_events() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("deferred-explicit", &identity, true));
    let destination = transport
        .add_destination_with_deferred_proofs(
            identity.clone(),
            DestinationName::new("lxmf", "delivery"),
        )
        .await;
    destination.lock().await.set_proof_strategy(ProofStrategy::All);
    let mut iface = transport.iface_manager().lock().await.new_channel(8);
    let mut legacy = transport.received_data_events();
    let mut deferred = transport.received_data_events_with_proofs();
    let ciphertext = crate::ratchets::encrypt_for_public_key(
        &identity.as_identity().public_key,
        identity.as_identity().address_hash.as_slice(),
        b"admission owned by application",
        OsRng,
    )
    .expect("encrypt");
    let packet = Packet {
        destination: destination.lock().await.desc.address_hash,
        data: PacketDataBuffer::new_from_slice(&ciphertext),
        ..Packet::default()
    };
    handle_data(&packet, iface.address, transport.handler.lock().await).await;
    let event = deferred.recv().await.expect("proof wrapper");
    assert_eq!(
        legacy.recv().await.expect("legacy event").data.as_slice(),
        event.received.data.as_slice()
    );
    assert!(iface.tx_channel.try_recv().is_err(), "no automatic proof even when All is set");
    let proof = event.proof.expect("deferred handle");
    let mut expired = proof.clone();
    expired.received_at -= INTERVAL_KEEP_PACKET_CACHED + Duration::from_secs(1);
    assert!(!transport.prove_destination_packet(expired).await);
    assert!(iface.tx_channel.try_recv().is_err());
    assert!(transport.prove_destination_packet(proof.clone()).await);
    let sent = iface.tx_channel.recv().await.expect("proof dispatch");
    assert_eq!(sent.packet.header.packet_type, PacketType::Proof);
    assert_eq!(&sent.packet.data.as_slice()[..HASH_SIZE], packet.hash().as_slice());
    destination.lock().await.set_deferred_proofs(false);
    assert!(!transport.prove_destination_packet(proof).await);
    assert!(iface.tx_channel.try_recv().is_err());
}
