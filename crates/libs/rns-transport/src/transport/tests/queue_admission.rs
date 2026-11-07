use super::*;
use crate::iface::InterfaceMode;
use crate::packet::{Header, PacketDataBuffer};
use rand_core::OsRng;
use std::collections::HashSet;

fn incoming(iface: AddressHash, seed: &[u8], context: PacketContext) -> RxMessage {
    RxMessage {
        address: iface,
        packet: Packet {
            header: Header { destination_type: DestinationType::Link, ..Default::default() },
            destination: AddressHash::new_from_rand(OsRng),
            context,
            data: PacketDataBuffer::new_from_slice(seed),
            ..Default::default()
        },
        source: IfaceSource::None,
    }
}

async fn admit(
    transport: &Transport,
    queues: &InboundQueues<QueuedInbound>,
    inbound: RxMessage,
) -> bool {
    let Some((class, candidate)) =
        preprocess_inbound_message(&transport.handler, &transport.iface_messages_tx, inbound).await
    else {
        return false;
    };
    enqueue_inbound_message(&transport.handler, queues, class, candidate).await
}

#[tokio::test]
async fn full_queue_preserves_current_and_previous_membership_and_proof_context() {
    for previous in [false, true] {
        let transport = Transport::new(TransportConfig::default());
        let iface = *transport.iface_manager().lock().await.new_channel(8).address();
        let queues = InboundQueues::new(InboundQueueLimits { data: 1, ..Default::default() });
        assert!(admit(&transport, &queues, incoming(iface, b"filler", PacketContext::None)).await);
        let rejected =
            incoming(iface, b"preexisting member with no proof context", PacketContext::Resource);
        let tracked = incoming(iface, b"preexisting tracked context", PacketContext::None).packet;
        let cache = transport.handler.lock().await.packet_cache.clone();
        let before = {
            let mut cache = cache.lock().await;
            cache.note_source(&tracked, iface);
            cache.restore_hashes([rejected.packet.hash()]);
            if previous {
                cache.restore_hashes((0_u64..500_001).map(|value| {
                    let mut hash = [0; HASH_SIZE];
                    hash[..8].copy_from_slice(&value.to_be_bytes());
                    Hash::new(hash)
                }));
                assert!(cache.maintain_duplicate_generations());
            }
            assert!(cache
                .proof_context_for_destination(&AddressHash::new_from_hash(&rejected.packet.hash()))
                .is_none());
            (
                cache.hashes().into_iter().collect::<HashSet<_>>(),
                cache.proof_context_for_destination(&AddressHash::new_from_hash(&tracked.hash())),
            )
        };
        assert!(!admit(&transport, &queues, rejected.clone()).await);
        let cache = cache.lock().await;
        assert!(cache.contains_hash(&rejected.packet.hash()));
        assert_eq!(
            cache.hashes().into_iter().collect::<HashSet<_>>(),
            before.0,
            "failed enqueue must neither insert nor promote membership"
        );
        assert_eq!(
            cache.proof_context_for_destination(&AddressHash::new_from_hash(&tracked.hash())),
            before.1
        );
        assert!(
            cache
                .proof_context_for_destination(&AddressHash::new_from_hash(&rejected.packet.hash()))
                .is_none(),
            "failed enqueue cannot invent proof context for an existing member"
        );
        assert_eq!(queues.snapshot().dropped(InboundTrafficClass::Data), 1);
        assert_eq!(queues.snapshot().height(InboundTrafficClass::Data), 1);
        transport.cancel.cancel();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn competing_ordinary_candidates_commit_once_without_counting_queue_overload() {
    let transport = Transport::new(TransportConfig::default());
    let iface = *transport.iface_manager().lock().await.new_channel(8).address();
    let queues = InboundQueues::new(InboundQueueLimits { data: 2, ..Default::default() });
    let mut inbound = incoming(iface, b"same ordinary candidate", PacketContext::None);
    inbound.packet.header.destination_type = DestinationType::Single;
    let first = preprocess_inbound_message(
        &transport.handler,
        &transport.iface_messages_tx,
        inbound.clone(),
    )
    .await
    .expect("first candidate");
    let second = preprocess_inbound_message(
        &transport.handler,
        &transport.iface_messages_tx,
        inbound.clone(),
    )
    .await
    .expect("second candidate before either commits");
    let (first, second) = tokio::join!(
        enqueue_inbound_message(&transport.handler, &queues, first.0, first.1),
        enqueue_inbound_message(&transport.handler, &queues, second.0, second.1),
    );
    assert_ne!(first, second);
    assert_eq!(queues.snapshot().height(InboundTrafficClass::Data), 1);
    assert_eq!(queues.snapshot().dropped(InboundTrafficClass::Data), 0);
    let cache = transport.handler.lock().await.packet_cache.clone();
    assert!(cache.lock().await.contains_hash(&inbound.packet.hash()));
    let filters = transport
        .iface_manager()
        .lock()
        .await
        .traffic_snapshots()
        .into_iter()
        .find(|entry| entry.address == iface)
        .expect("interface counters")
        .packet_filter_hits;
    assert_eq!(filters, 1);
    transport.cancel.cancel();
}

#[tokio::test]
async fn final_admission_uses_current_shared_mode_and_cache_membership() {
    for attached in [false, true] {
        let transport = Transport::new(TransportConfig::default());
        let iface = *transport.iface_manager().lock().await.new_channel(8).address();
        let queues = InboundQueues::new(InboundQueueLimits::default());
        let mut inbound = incoming(iface, b"current mode at commit", PacketContext::None);
        inbound.packet.header.destination_type = DestinationType::Single;
        let candidate = preprocess_inbound_message(
            &transport.handler,
            &transport.iface_messages_tx,
            inbound.clone(),
        )
        .await
        .expect("initial fresh candidate");
        transport
            .handler
            .lock()
            .await
            .packet_cache
            .lock()
            .await
            .restore_hashes([inbound.packet.hash()]);
        transport.set_connected_to_shared_instance(attached).await;
        assert_eq!(
            enqueue_inbound_message(&transport.handler, &queues, candidate.0, candidate.1).await,
            attached
        );
        assert_eq!(queues.snapshot().height(InboundTrafficClass::Data), usize::from(attached));
        assert_eq!(queues.snapshot().dropped(InboundTrafficClass::Data), 0);
        transport.cancel.cancel();
    }
}

#[tokio::test]
async fn foreign_transport_is_filtered_before_path_request_admission() {
    let mut config = TransportConfig::default();
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let iface = *transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role_and_mode(8, IfaceRole::Unicast, InterfaceMode::AccessPoint)
        .address();
    let destination = AddressHash::new_from_rand(OsRng);
    let mut requests = PathRequests::new("", None, 16, 16, 30);
    let mut packet =
        requests.generate(&destination, Some(vec![0x57; crate::hash::ADDRESS_HASH_SIZE]));
    packet.header.header_type = crate::packet::HeaderType::Type2;
    packet.transport = Some(AddressHash::new_from_rand(OsRng));
    let rejected = RxMessage { address: iface, packet: packet.clone(), source: IfaceSource::None };
    assert!(preprocess_inbound_message(&transport.handler, &transport.iface_messages_tx, rejected)
        .await
        .is_none());
    packet.transport = Some(transport.internal_identity().await);
    let accepted = RxMessage { address: iface, packet, source: IfaceSource::None };
    let queues = InboundQueues::new(InboundQueueLimits::default());
    assert!(
        admit(&transport, &queues, accepted).await,
        "foreign transport must not reserve the exact path-request key"
    );
    assert_eq!(queues.snapshot().height(InboundTrafficClass::PathRequest), 1);
    transport.cancel.cancel();
}
