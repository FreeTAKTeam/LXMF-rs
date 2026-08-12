#[tokio::test]
async fn responder_proof_does_not_wait_for_unrelated_inbound_link() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut initiator = Link::new(destination, tx.clone());
    let request = initiator.request();
    let mut responder =
        Link::new_from_request(&request, signer.sign_key().clone(), destination, tx.clone())
            .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        initiator.handle_packet(&responder.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
    assert!(matches!(
        responder.handle_packet(&initiator.create_rtt(), iface),
        crate::destination::link::LinkHandleResult::None
    ));
    initiator.register_channel_handler(0x55AA, |_| true);

    let link_id = *responder.id();
    let responder = Arc::new(Mutex::new(responder));
    let unrelated_identity = PrivateIdentity::new_from_rand(OsRng);
    let unrelated_destination = crate::destination::DestinationDesc {
        identity: *unrelated_identity.as_identity(),
        address_hash: *unrelated_identity.address_hash(),
        name: DestinationName::new("lxmf", "unrelated"),
    };
    let unrelated = Arc::new(Mutex::new(Link::new(unrelated_destination, tx)));
    let unrelated_id = *unrelated.lock().await.id();
    {
        let mut handler = handler.lock().await;
        handler.in_links.insert(link_id, responder.clone());
        handler.in_links.insert(unrelated_id, unrelated.clone());
    }

    let (sequence, packet) = responder
        .lock()
        .await
        .send_channel_message(0x55AA, b"responder-message".to_vec())
        .expect("channel message");
    let proof = match initiator.handle_packet(&packet, iface) {
        crate::destination::link::LinkHandleResult::Proof(proof) => proof,
        _ => panic!("channel packet should generate proof"),
    };

    let _unrelated_guard = unrelated.lock().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        handle_proof(proof, handler, iface),
    )
    .await
    .expect("proof handling must not lock an unrelated inbound link");

    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Delivered
    );
}
