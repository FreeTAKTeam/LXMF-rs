async fn commit_local_link_packet(transport: &Transport, packet: Packet, iface: AddressHash) {
    let queues = InboundQueues::new(InboundQueueLimits::default());
    let (class, candidate) = preprocess_inbound_message(
        &transport.handler,
        &transport.iface_messages_tx,
        RxMessage { address: iface, packet, source: IfaceSource::None },
    )
    .await
    .expect("candidate passes wire admission");
    assert!(
        super::inbound_processing::enqueue_inbound_message(
            &transport.handler,
            &queues,
            class,
            candidate,
        )
        .await,
        "candidate passes final admission"
    );
    super::inbound_processing::process_inbound_message(
        transport.handler.clone(),
        queues.try_dequeue().expect("owned queued packet"),
    )
    .await;
}

#[tokio::test]
async fn wrong_interface_resource_data_does_not_reject_and_identical_correct_replay_can_finish() {
    use crate::resource::{build_link_packet, ResourceOutcome};
    for inbound in [false, true] {
        let (transport, retained, peer, mut channel, _) = lifecycle_fixture(inbound).await;
        let (hash, completion) = observed_lifecycle_resource(&transport, &retained).await;
        let _advertisement = channel.tx_channel.recv().await.expect("actual Resource advertised");
        let packet = build_link_packet(
            &peer,
            PacketType::Data,
            PacketContext::ResourceReceiverCancel,
            hash.as_slice(),
        )
        .expect("authenticated native Resource cancel");
        let hash = packet.hash();
        let wrong_iface = *transport.iface_manager().lock().await.new_channel(8).address();
        commit_local_link_packet(&transport, packet.clone(), wrong_iface).await;
        assert_eq!(
            completion.current(),
            None,
            "wrong-interface control Data must not mutate Resource state"
        );
        let cache = transport.handler.lock().await.packet_cache.clone();
        assert!(
            !cache.lock().await.contains_hash(&hash),
            "wrong-interface Data forgets sticky membership"
        );
        commit_local_link_packet(&transport, packet, *channel.address()).await;
        assert_eq!(completion.current(), Some(ResourceOutcome::Rejected));
        assert!(cache.lock().await.contains_hash(&hash));
        transport.cancel.cancel();
    }
}
