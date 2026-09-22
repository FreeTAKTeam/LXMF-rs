use super::*;
use crate::iface::InterfaceMode;
use rand_core::OsRng;

fn message(packet_type: PacketType, destination: AddressHash) -> RxMessage {
    RxMessage {
        address: AddressHash::new_from_rand(OsRng),
        packet: Packet {
            header: crate::packet::Header { packet_type, ..Default::default() },
            destination,
            ..Default::default()
        },
        source: IfaceSource::None,
    }
}

async fn processed_hops(transport: &Transport, iface: AddressHash, wire_hops: u8) -> u8 {
    let mut inbound = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
    inbound.address = iface;
    inbound.packet.header.hops = wire_hops;
    preprocess_inbound_message(&transport.get_handler(), &transport.iface_messages_tx, inbound)
        .await
        .expect("inbound packet should be queued")
        .1
        .message
        .packet
        .header
        .hops
}

#[test]
fn rns_1_5_ingress_classifies_management_traffic_before_queueing() {
    let path_request = AddressHash::new_from_rand(OsRng);
    let ordinary = AddressHash::new_from_rand(OsRng);

    assert_eq!(
        inbound_traffic_class(&message(PacketType::Data, ordinary), path_request),
        InboundTrafficClass::Data
    );
    assert_eq!(
        inbound_traffic_class(&message(PacketType::Announce, ordinary), path_request),
        InboundTrafficClass::Announce
    );
    assert_eq!(
        inbound_traffic_class(&message(PacketType::Data, path_request), path_request),
        InboundTrafficClass::PathRequest
    );
}

#[tokio::test]
async fn rns_1_5_ingress_plain_and_group_hop_filter_uses_wire_hops() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();

    for destination_type in [DestinationType::Plain, DestinationType::Group] {
        for wire_hops in [0, 1] {
            let mut inbound = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
            inbound.address = iface;
            inbound.packet.header.destination_type = destination_type;
            inbound.packet.header.hops = wire_hops;
            assert!(
                preprocess_inbound_message(&handler, &transport.iface_messages_tx, inbound)
                    .await
                    .is_some(),
                "wire hops {wire_hops} must be accepted for {destination_type:?}"
            );
        }

        let mut transported = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
        transported.address = iface;
        transported.packet.header.destination_type = destination_type;
        transported.packet.header.hops = 2;
        assert!(
            preprocess_inbound_message(&handler, &transport.iface_messages_tx, transported,)
                .await
                .is_none(),
            "wire hops 2 must be rejected for {destination_type:?}"
        );
    }
}

#[tokio::test]
async fn shared_instance_receive_removes_only_the_local_boundary_hop() {
    let mut config = TransportConfig::default();
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let (shared_iface, ordinary_iface) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let shared_iface = *manager.new_channel(8).address();
        let ordinary_iface = *manager.new_channel(8).address();
        assert!(manager.set_shared_instance(shared_iface, true));
        (shared_iface, ordinary_iface)
    };

    assert_eq!(processed_hops(&transport, shared_iface, 0).await, 0);
    assert_eq!(processed_hops(&transport, shared_iface, 1).await, 1);
    assert_eq!(processed_hops(&transport, ordinary_iface, 0).await, 1);
}

#[tokio::test]
async fn shared_instance_parent_does_not_remove_child_boundary_hop() {
    let mut config = TransportConfig::default();
    config.set_connected_to_shared_instance(true);
    let transport = Transport::new(config);
    let (parent, child) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        let parent = *manager.new_channel(8).address();
        let child = *manager.new_channel(8).address();
        assert!(manager.set_shared_instance(parent, true));
        assert!(manager.inherit_runtime_config(parent, child));
        (parent, child)
    };

    assert_eq!(processed_hops(&transport, parent, 0).await, 1);
    assert_eq!(processed_hops(&transport, child, 0).await, 0);
}

#[tokio::test]
async fn rns_1_5_ingress_full_queue_does_not_poison_packet_retry() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let queues = InboundQueues::new(InboundQueueLimits {
        data: 1,
        announce: 1,
        path_request: 1,
        ingress_limited: 1,
    });

    let mut first = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
    first.address = iface;
    let (first_class, first) =
        preprocess_inbound_message(&handler, &transport.iface_messages_tx, first)
            .await
            .expect("first packet");
    queues.enqueue(first_class, first).expect("queue first packet");

    let mut retry = message(PacketType::Data, AddressHash::new_from_rand(OsRng));
    retry.address = iface;
    retry.packet.data = PacketDataBuffer::new_from_slice(b"retry after queue pressure");
    let (retry_class, rejected) =
        preprocess_inbound_message(&handler, &transport.iface_messages_tx, retry.clone())
            .await
            .expect("first retry attempt");
    let full = queues.enqueue(retry_class, rejected).expect_err("data queue must be full");
    rollback_rejected_inbound(&handler, &full.item).await;
    let _ = queues.try_dequeue().expect("free queue capacity");

    assert!(
        preprocess_inbound_message(&handler, &transport.iface_messages_tx, retry).await.is_some(),
        "queue-full rejection must not poison the packet hash cache"
    );
}

#[tokio::test]
async fn rns_1_5_ingress_path_request_deduplication_reaches_the_scoped_cache() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let path_request_destination = handler.lock().await.fixed_dest_path_requests;

    let (iface_a_channel, iface_b_channel) = {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        (
            manager.new_channel_with_role_and_mode(
                16,
                IfaceRole::Unicast,
                InterfaceMode::AccessPoint,
            ),
            manager.new_channel_with_role_and_mode(
                16,
                IfaceRole::Unicast,
                InterfaceMode::AccessPoint,
            ),
        )
    };
    let iface_a = *iface_a_channel.address();
    let iface_b = *iface_b_channel.address();

    let destination = AddressHash::new_from_rand(OsRng);
    let mut generator = PathRequests::new("", None, 16, 16, 30);

    let first_packet =
        generator.generate(&destination, Some(vec![0x5a; crate::hash::ADDRESS_HASH_SIZE]));
    let queued = preprocess_inbound_message(
        &handler,
        &transport.iface_messages_tx,
        RxMessage { address: iface_a, packet: first_packet, source: IfaceSource::None },
    )
    .await
    .expect("first path request must be queued");
    assert_eq!(queued.0, InboundTrafficClass::PathRequest);
    process_inbound_message(handler.clone(), queued.1).await;

    let second_packet =
        generator.generate(&destination, Some(vec![0x5b; crate::hash::ADDRESS_HASH_SIZE]));
    assert!(
        preprocess_inbound_message(
            &handler,
            &transport.iface_messages_tx,
            RxMessage { address: iface_b, packet: second_packet, source: IfaceSource::None },
        )
        .await
        .is_none(),
        "same-destination request must batch before entering the queue"
    );

    let guard = handler.lock().await;
    assert_eq!(guard.iface_manager.lock().await.mode(&iface_a), Some(InterfaceMode::AccessPoint));
    drop(guard);

    assert_eq!(
        handler.lock().await.path_requests.discovery_requesters(&destination),
        vec![iface_a, iface_b]
    );
    assert_eq!(generator.generate(&destination, None).destination, path_request_destination);
}
