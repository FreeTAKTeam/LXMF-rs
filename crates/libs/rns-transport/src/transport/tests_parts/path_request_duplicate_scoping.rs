use super::path_requests::PathRequests;

#[tokio::test]
async fn local_path_response_duplicate_scoping_reaches_requester_and_iface_policy() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let mut transport = Transport::new(config);

    let local_destination = transport
        .add_destination(
            PrivateIdentity::new_from_rand(OsRng),
            DestinationName::new("lxmf", "delivery"),
        )
        .await;
    let destination = local_destination.lock().await.desc.address_hash;
    let handler = transport.get_handler();

    let (mut iface_a_channel, mut iface_b_channel) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(16), manager.new_channel(16))
    };
    let iface_a = *iface_a_channel.address();
    let iface_b = *iface_b_channel.address();

    let requester_a = AddressHash::new_from_rand(OsRng);
    let requester_b = AddressHash::new_from_rand(OsRng);
    let mut sender_a = PathRequests::new("", Some(requester_a), 16, 16, 30);
    let mut sender_b = PathRequests::new("", Some(requester_b), 16, 16, 30);
    let tag = vec![0x58; crate::hash::ADDRESS_HASH_SIZE];
    let request_a = sender_a.generate(&destination, Some(tag.clone()));
    let request_b = sender_b.generate(&destination, Some(tag));

    {
        let mut guard = handler.lock().await;
        handle_path_request(&request_a, &mut guard, iface_a).await;
    }
    let first = timeout(Duration::from_millis(200), iface_a_channel.tx_channel.recv())
        .await
        .expect("first request should receive a local path response")
        .expect("first path response message");
    assert!(matches!(first.tx_type, TxMessageType::Direct(iface) if iface == iface_a));
    assert_eq!(first.packet.destination, destination);
    assert_eq!(first.packet.context, PacketContext::PathResponse);

    {
        let mut guard = handler.lock().await;
        handle_path_request(&request_a, &mut guard, iface_a).await;
    }
    assert!(
        matches!(
            iface_a_channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "exact duplicate requester/interface should still be suppressed"
    );

    {
        let mut guard = handler.lock().await;
        handle_path_request(&request_b, &mut guard, iface_a).await;
    }
    let second = timeout(Duration::from_millis(200), iface_a_channel.tx_channel.recv())
        .await
        .expect("distinct requester should receive a local path response")
        .expect("second path response message");
    assert!(matches!(second.tx_type, TxMessageType::Direct(iface) if iface == iface_a));
    assert_eq!(second.packet.destination, destination);
    assert_eq!(second.packet.context, PacketContext::PathResponse);

    {
        let mut guard = handler.lock().await;
        handle_path_request(&request_a, &mut guard, iface_b).await;
    }
    let third = timeout(Duration::from_millis(200), iface_b_channel.tx_channel.recv())
        .await
        .expect("distinct iface should receive a local path response")
        .expect("third path response message");
    assert!(matches!(third.tx_type, TxMessageType::Direct(iface) if iface == iface_b));
    assert_eq!(third.packet.destination, destination);
    assert_eq!(third.packet.context, PacketContext::PathResponse);
}
