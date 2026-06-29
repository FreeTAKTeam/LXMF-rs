use crate::packet::PropagationType;

#[tokio::test]
async fn unknown_path_request_is_answered_when_matching_announce_arrives() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let (mut learned_channel, mut requester_channel) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (manager.new_channel(16), manager.new_channel(16))
    };
    let learned_iface = *learned_channel.address();
    let requester_iface = *requester_channel.address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    announce.header.hops = 3;
    let destination = announce.destination;
    let cached_data = announce.data.clone();

    let path_request = {
        let mut guard = handler.lock().await;
        guard.path_requests.generate(&destination, Some(vec![0x91; crate::hash::ADDRESS_HASH_SIZE]))
    };

    {
        let mut guard = handler.lock().await;
        handle_path_request(&path_request, &mut guard, requester_iface).await;
    }

    let recursive = timeout(Duration::from_millis(200), learned_channel.tx_channel.recv())
        .await
        .expect("recursive path request should be forwarded")
        .expect("recursive path request message");
    assert!(
        matches!(recursive.tx_type, TxMessageType::Broadcast(Some(iface)) if iface == requester_iface),
        "recursive discovery request should exclude the original requester iface"
    );
    assert_eq!(recursive.packet.destination, path_request.destination);
    assert_eq!(recursive.packet.data.as_slice(), path_request.data.as_slice());
    assert!(matches!(
        requester_channel.tx_channel.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let response = timeout(Duration::from_millis(200), requester_channel.tx_channel.recv())
        .await
        .expect("matching announce should answer waiting discovery request")
        .expect("path response message");
    assert!(
        matches!(response.tx_type, TxMessageType::Direct(iface) if iface == requester_iface),
        "waiting discovery path responses should be direct to the original requester"
    );
    assert_eq!(response.packet.destination, destination);
    assert_eq!(response.packet.header.header_type, HeaderType::Type2);
    assert_eq!(response.packet.header.propagation_type, PropagationType::Transport);
    assert_eq!(response.packet.header.hops, 3);
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    assert_eq!(response.packet.transport, Some(*local_identity.address_hash()));
    assert_eq!(response.packet.data.as_slice(), cached_data.as_slice());

    handle_announce(
        &announce,
        handler.lock().await,
        learned_iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    assert!(
        matches!(
            requester_channel.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "a consumed discovery request should not be answered again"
    );
}
